//! # Sandbox — Domain-Agnostic Simulation Substrate for External Solvers
//!
//! ## Table of Contents
//! - Property          — named observable fact about a world state
//! - WorldStateError   — errors from WorldState operations
//! - WorldState        — the contract every domain must implement
//! - SandboxRecord     — one action + its observation
//! - Sandbox           — isolated simulation fork for a single hypothesis
//! - SimulationBudget  — resource limits across all branches
//! - BranchState       — lifecycle of one search branch
//! - HypothesisNode    — one search node in the tree
//! - HypothesisTree    — concurrent branch manager
//!
//! ## Design Contract
//!
//! EustressEngine provides the substrate; external solvers provide the strategy.
//!
//! ```text
//! EustressEngine (this module)         External Solver (Vortex, etc.)
//! ────────────────────────────         ──────────────────────────────
//! WorldState trait ─────────────────►  Grid2D, Scene3D, GameState impls
//! Sandbox::new(state) ──────────────►  branch creation
//! Sandbox::apply(action) ───────────►  step the simulation
//! Sandbox::score_against(goal) ─────►  evaluate
//! HypothesisTree ───────────────────►  branch management
//! SimulationBudget ─────────────────►  resource control
//!                                      solve<W: WorldState>() lives here
//! ```
//!
//! ## Sparse Checkout
//!
//! External models that use EustressEngine as a submodule need only:
//!
//! ```
//! eustress/Cargo.toml
//! eustress/crates/common/
//! eustress/crates/embedvec/
//! eustress/.patches/iggy_common-0.9.0/
//! ```
//!
//! See `docs/SPARSE_CHECKOUT.md` for the full `git sparse-checkout` incantation.
//!
//! ## The Unified Solve Loop (lives in the external solver, not here)
//!
//! ```rust,ignore
//! fn solve<W: WorldState>(
//!     world: &W, goal: &W,
//!     causal_graph: &mut CausalGraph,
//!     budget: &SimulationBudget,
//! ) -> Option<Vec<W::Action>> {
//!     let props = world.analyze();
//!     let hypotheses = causal_graph.suggest_hypotheses(&props, goal, budget.max_active_branches);
//!     let mut tree = HypothesisTree::new(budget.clone());
//!     for h in hypotheses { tree.add_branch(h); }
//!     while tree.has_active_branches() && !tree.budget_exhausted() {
//!         // step, score, branch/prune/commit
//!     }
//!     tree.best_solution()
//! }
//! ```

use crate::iggy_delta::SceneDelta;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::{Duration, Instant};

// ─────────────────────────────────────────────────────────────────────────────
// Property
// ─────────────────────────────────────────────────────────────────────────────

/// A named observable fact about a world state.
///
/// Properties are computed by `WorldState::analyze()` and consumed by the
/// causal graph's hypothesis generator.  They are intentionally untyped at
/// the trait boundary — the solver interprets the value.
///
/// Examples:
/// - `Property { name: "has_horizontal_symmetry", value: PropertyValue::Bool(true) }`
/// - `Property { name: "object_count",            value: PropertyValue::Int(7)     }`
/// - `Property { name: "mean_color",              value: PropertyValue::Float(3.4) }`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Property {
    /// Machine-readable identifier.  Dot-separated namespacing recommended:
    /// `"grid.symmetry.horizontal"`, `"scene.contact_graph.stable"`.
    pub name: String,
    /// The computed value.
    pub value: PropertyValue,
    /// Confidence the detector has in this reading [0.0–1.0].
    pub confidence: f32,
}

/// Typed value for a `Property`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PropertyValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Vector(Vec<f64>),
}

impl Property {
    pub fn bool(name: impl Into<String>, value: bool) -> Self {
        Self { name: name.into(), value: PropertyValue::Bool(value), confidence: 1.0 }
    }

    pub fn int(name: impl Into<String>, value: i64) -> Self {
        Self { name: name.into(), value: PropertyValue::Int(value), confidence: 1.0 }
    }

    pub fn float(name: impl Into<String>, value: f64) -> Self {
        Self { name: name.into(), value: PropertyValue::Float(value), confidence: 1.0 }
    }

    pub fn text(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self { name: name.into(), value: PropertyValue::Text(value.into()), confidence: 1.0 }
    }

    pub fn with_confidence(mut self, c: f32) -> Self {
        self.confidence = c.clamp(0.0, 1.0);
        self
    }

    /// Check whether this property's name matches `query` (exact or prefix match).
    pub fn matches(&self, query: &str) -> bool {
        self.name == query || self.name.starts_with(&format!("{query}."))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// WorldStateError
// ─────────────────────────────────────────────────────────────────────────────

/// Errors from WorldState operations.
#[derive(Debug, thiserror::Error)]
pub enum WorldStateError {
    #[error("serialization failed: {0}")]
    SerializationFailed(String),
    #[error("deserialization failed: {0}")]
    DeserializationFailed(String),
    #[error("invalid action: {0}")]
    InvalidAction(String),
    #[error("simulation step failed: {0}")]
    StepFailed(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// WorldState
// ─────────────────────────────────────────────────────────────────────────────

/// The contract every simulation domain must implement to participate in the
/// universal learning loop.
///
/// # Implementations
///
/// - `Grid2D`    — ARC-AGI-3 2D grid tasks  (in Vortex)
/// - `Scene3D`   — EustressEngine 3D scenes (in eustress-engine)
/// - `GameState` — Rune-scripted game levels (in Vortex)
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` because sandboxes run concurrently
/// across `tokio` tasks (one per branch in the hypothesis tree).
pub trait WorldState: Clone + Send + Sync + 'static {
    /// The action type for this domain (DSL op, physics command, game verb).
    type Action: Clone + Debug + Send + Sync + Serialize;

    /// The observation produced by applying one action.
    type Observation: Clone + Debug + Send + Sync;

    /// A score that can be compared for "better / worse" (higher = better).
    type Score: Clone + Debug + PartialOrd + Into<f32>;

    // ── Observation ────────────────────────────────────────────────────────

    /// Compute observable properties of this state.
    ///
    /// Called by the causal graph to match conditions for hypothesis generation.
    /// Should be O(1)–O(n) in scene complexity; avoid heavy computation here.
    fn analyze(&self) -> Vec<Property>;

    /// All actions currently valid in this state.
    ///
    /// The solver uses this to enumerate the search space.  Return only
    /// contextually valid actions (e.g., don't return `rotate_cw` if the
    /// grid is already rotationally symmetric with the goal).
    fn available_actions(&self) -> Vec<Self::Action>;

    // ── Transition ─────────────────────────────────────────────────────────

    /// Apply `action` and return `(next_state, observation)`.
    ///
    /// Must be pure: does not mutate `self`.  The sandbox calls this
    /// in a cloned copy of the state.
    fn apply(&self, action: &Self::Action) -> Result<(Self, Self::Observation), WorldStateError>;

    // ── Evaluation ─────────────────────────────────────────────────────────

    /// Score `self` against `goal`. Higher = closer to goal.
    ///
    /// For ARC: `exact_match` (bool → 0.0 or 1.0) + `cell_accuracy` ([0,1]).
    /// For physics: continuous goal satisfaction measure.
    fn score_against(&self, goal: &Self) -> Self::Score;

    // ── Interop ────────────────────────────────────────────────────────────

    /// Compute the set of `SceneDelta`s that transform `self` into `other`.
    ///
    /// Used to publish state changes onto the Iggy delta stream so the
    /// `SalienceFilter` and `MemoryTierController` can process them.
    fn diff_to_deltas(&self, other: &Self) -> Vec<SceneDelta>;

    /// Serialize to bytes (for Iggy publishing and RocksDB storage).
    fn to_bytes(&self) -> Result<Vec<u8>, WorldStateError>;

    /// Deserialize from bytes.
    fn from_bytes(bytes: &[u8]) -> Result<Self, WorldStateError>
    where
        Self: Sized;
}

// ─────────────────────────────────────────────────────────────────────────────
// SandboxRecord
// ─────────────────────────────────────────────────────────────────────────────

/// One step in a sandbox execution: the action taken and its observation.
#[derive(Debug, Clone)]
pub struct SandboxRecord<W: WorldState> {
    /// Step index (0-based).
    pub step: usize,
    /// Action applied at this step.
    pub action: W::Action,
    /// Observation produced.
    pub observation: W::Observation,
    /// Score after this step.
    pub score: Option<W::Score>,
    /// Wall-clock time this step took.
    pub elapsed_us: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Sandbox
// ─────────────────────────────────────────────────────────────────────────────

/// An isolated simulation fork for testing one hypothesis.
///
/// Each `Sandbox` owns a clone of the initial world state and advances it
/// independently without affecting any other sandbox or the live scene.
/// This is how hypothesis branches work: create one sandbox per branch,
/// step them independently, score the results.
///
/// ```rust,ignore
/// let sandbox = Sandbox::new(initial_state.clone(), goal.clone());
/// for action in hypothesis.program {
///     let scored = sandbox.apply(action)?;
///     if scored.score >= 1.0 { return Some(sandbox.history()); }
/// }
/// let score = sandbox.final_score();
/// ```
pub struct Sandbox<W: WorldState> {
    /// Current state of this branch.
    state: W,
    /// Goal state we are trying to reach.
    goal: W,
    /// Ordered history of (action, observation, score).
    history: Vec<SandboxRecord<W>>,
    /// Total steps taken.
    pub step_count: usize,
}

impl<W: WorldState> Sandbox<W> {
    /// Create a new sandbox with a cloned initial state and goal.
    pub fn new(initial: W, goal: W) -> Self {
        Self {
            state: initial,
            goal,
            history: Vec::new(),
            step_count: 0,
        }
    }

    /// Current state of this sandbox.
    pub fn current(&self) -> &W {
        &self.state
    }

    /// Apply an action, advance the state, record the step.
    ///
    /// Returns the score after this step.
    pub fn apply(&mut self, action: W::Action) -> Result<W::Score, WorldStateError> {
        let t0 = Instant::now();
        let (next_state, observation) = self.state.apply(&action)?;
        let elapsed_us = t0.elapsed().as_micros() as u64;
        let score = next_state.score_against(&self.goal);
        self.history.push(SandboxRecord {
            step: self.step_count,
            action,
            observation,
            score: Some(score.clone()),
            elapsed_us,
        });
        self.state = next_state;
        self.step_count += 1;
        Ok(score)
    }

    /// Score the current state against the goal.
    pub fn current_score(&self) -> W::Score {
        self.state.score_against(&self.goal)
    }

    /// Float score of the current state [0.0–1.0].
    pub fn score_f32(&self) -> f32 {
        self.current_score().into()
    }

    /// Ordered list of actions taken so far.
    pub fn action_sequence(&self) -> Vec<W::Action> {
        self.history.iter().map(|r| r.action.clone()).collect()
    }

    /// Full step history.
    pub fn history(&self) -> &[SandboxRecord<W>] {
        &self.history
    }

    /// Deltas from initial state → current state (for Iggy publishing).
    pub fn accumulated_deltas(&self) -> Vec<SceneDelta> {
        // We'd need the initial state here to diff; in practice the caller
        // captures the initial state before creating the sandbox.
        Vec::new() // placeholder — caller should diff(initial, current)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SimulationBudget
// ─────────────────────────────────────────────────────────────────────────────

/// Resource limits for the hypothesis search.
///
/// Prevents unbounded exploration. The solver respects these; the engine
/// enforces them by checking `HypothesisTree::budget_exhausted()` each
/// iteration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationBudget {
    /// Maximum total simulation steps across all branches combined.
    pub max_total_steps: usize,
    /// Maximum depth (program length) per branch.
    pub max_branch_depth: usize,
    /// Maximum number of concurrently active (non-completed) branches.
    pub max_active_branches: usize,
    /// Minimum score to avoid being pruned.
    pub prune_threshold: f32,
    /// Optional wall-clock time limit across the entire search.
    pub time_limit: Option<Duration>,
}

impl Default for SimulationBudget {
    fn default() -> Self {
        Self {
            max_total_steps: 10_000,
            max_branch_depth: 32,
            max_active_branches: 16,
            prune_threshold: 0.05,
            time_limit: Some(Duration::from_secs(30)),
        }
    }
}

impl SimulationBudget {
    pub fn tight() -> Self {
        Self {
            max_total_steps: 1_000,
            max_branch_depth: 8,
            max_active_branches: 4,
            prune_threshold: 0.20,
            time_limit: Some(Duration::from_secs(5)),
        }
    }

    pub fn generous() -> Self {
        Self {
            max_total_steps: 100_000,
            max_branch_depth: 64,
            max_active_branches: 64,
            prune_threshold: 0.01,
            time_limit: Some(Duration::from_secs(300)),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// BranchState
// ─────────────────────────────────────────────────────────────────────────────

/// Lifecycle state of one hypothesis branch.
#[derive(Debug, Clone, PartialEq)]
pub enum BranchState {
    /// Waiting to be assigned a sandbox and simulated.
    Pending,
    /// Currently executing in a sandbox.
    Simulating,
    /// Simulation complete, score above prune_threshold but below 1.0.
    Partial { score: f32 },
    /// Score above commit_threshold — solution candidate.
    Committed { score: f32 },
    /// Score below prune_threshold — dead end, stop exploring.
    Pruned { score: f32 },
}

impl BranchState {
    pub fn score(&self) -> Option<f32> {
        match self {
            BranchState::Partial { score }
            | BranchState::Committed { score }
            | BranchState::Pruned { score } => Some(*score),
            _ => None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, BranchState::Committed { .. } | BranchState::Pruned { .. })
    }

    pub fn is_active(&self) -> bool {
        matches!(self, BranchState::Pending | BranchState::Simulating | BranchState::Partial { .. })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Hypothesis
// ─────────────────────────────────────────────────────────────────────────────

/// A candidate program to test in a sandbox.
///
/// Created by the external solver's hypothesis generator (informed by the
/// `CausalGraph`). The solver populates `source_law` when the hypothesis
/// was derived from a causal law, so the causal graph can credit the right
/// law when the hypothesis succeeds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hypothesis {
    /// Human-readable description (for logging).
    pub description: String,
    /// Prior probability this hypothesis succeeds [0.0–1.0].
    /// Informed by `CausalGraph::confidence` for the suggested law.
    pub prior_confidence: f32,
    /// Which causal law suggested this hypothesis (for credit assignment).
    pub source_law: Option<String>,
    /// Which properties of the input motivated this hypothesis.
    pub motivating_properties: Vec<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// HypothesisNode
// ─────────────────────────────────────────────────────────────────────────────

/// One node in the hypothesis search tree.
#[derive(Debug)]
pub struct HypothesisNode {
    /// Unique branch identifier.
    pub id: BranchId,
    /// Parent branch (None for root-level branches).
    pub parent: Option<BranchId>,
    /// The hypothesis this branch is testing.
    pub hypothesis: Hypothesis,
    /// Current lifecycle state.
    pub state: BranchState,
    /// Depth in the tree (0 = top-level hypothesis).
    pub depth: usize,
    /// IDs of child branches (spawned when this branch had a partial score).
    pub children: Vec<BranchId>,
    /// Steps consumed by this branch.
    pub steps_used: usize,
}

/// Opaque branch identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BranchId(pub u64);

impl BranchId {
    fn next() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        BranchId(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// HypothesisTree
// ─────────────────────────────────────────────────────────────────────────────

/// Manages the set of concurrent hypothesis branches.
///
/// The external solver calls:
/// 1. `add_branch(hypothesis)` — for each top-level candidate
/// 2. `next_pending()` — to get the highest-priority branch to simulate next
/// 3. `score_branch(id, score)` — after running the sandbox
/// 4. `spawn_child(parent_id, refined_hypothesis)` — when partial, explore refinements
/// 5. `best_committed()` — to retrieve the winning solution
///
/// The tree is the "quantum" search space: all branches exist simultaneously
/// until scored, at which point they collapse to committed or pruned.
#[derive(Debug)]
pub struct HypothesisTree {
    nodes: HashMap<BranchId, HypothesisNode>,
    pub budget: SimulationBudget,
    pub total_steps_used: usize,
    start_time: Instant,
    /// Score threshold above which a branch is committed as a solution.
    pub commit_threshold: f32,
}

impl HypothesisTree {
    pub fn new(budget: SimulationBudget) -> Self {
        Self {
            nodes: HashMap::new(),
            commit_threshold: 0.99,
            budget,
            total_steps_used: 0,
            start_time: Instant::now(),
        }
    }

    /// Add a top-level branch.  Returns the new BranchId.
    pub fn add_branch(&mut self, hypothesis: Hypothesis) -> BranchId {
        let id = BranchId::next();
        self.nodes.insert(id, HypothesisNode {
            id,
            parent: None,
            hypothesis,
            state: BranchState::Pending,
            depth: 0,
            children: Vec::new(),
            steps_used: 0,
        });
        id
    }

    /// Spawn a child branch from a partial result.  Returns the child's BranchId.
    pub fn spawn_child(
        &mut self,
        parent_id: BranchId,
        hypothesis: Hypothesis,
    ) -> Option<BranchId> {
        let parent_depth = self.nodes.get(&parent_id)?.depth;
        if parent_depth + 1 > self.budget.max_branch_depth {
            return None; // depth budget exhausted
        }
        let id = BranchId::next();
        self.nodes.insert(id, HypothesisNode {
            id,
            parent: Some(parent_id),
            hypothesis,
            state: BranchState::Pending,
            depth: parent_depth + 1,
            children: Vec::new(),
            steps_used: 0,
        });
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            parent.children.push(id);
        }
        Some(id)
    }

    /// Return the pending branch with the highest `prior_confidence`.
    /// The solver should simulate this next.
    pub fn next_pending(&self) -> Option<BranchId> {
        self.nodes
            .values()
            .filter(|n| n.state == BranchState::Pending)
            .max_by(|a, b| {
                a.hypothesis.prior_confidence
                    .partial_cmp(&b.hypothesis.prior_confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|n| n.id)
    }

    /// Mark a branch as currently simulating.
    pub fn mark_simulating(&mut self, id: BranchId) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.state = BranchState::Simulating;
        }
    }

    /// Record the final score for a branch.  Transitions it to Committed or Pruned.
    pub fn score_branch(&mut self, id: BranchId, score: f32, steps: usize) {
        self.total_steps_used += steps;
        if let Some(node) = self.nodes.get_mut(&id) {
            node.steps_used = steps;
            node.state = if score >= self.commit_threshold {
                BranchState::Committed { score }
            } else if score < self.budget.prune_threshold {
                BranchState::Pruned { score }
            } else {
                BranchState::Partial { score }
            };
        }
    }

    /// Whether there are any pending or simulating branches.
    pub fn has_active_branches(&self) -> bool {
        self.nodes.values().any(|n| n.state.is_active())
    }

    /// Whether the total step budget or time limit has been exhausted.
    pub fn budget_exhausted(&self) -> bool {
        if self.total_steps_used >= self.budget.max_total_steps {
            return true;
        }
        if let Some(limit) = self.budget.time_limit {
            if self.start_time.elapsed() >= limit {
                return true;
            }
        }
        false
    }

    /// The best committed branch (highest score), if any.
    pub fn best_committed(&self) -> Option<&HypothesisNode> {
        self.nodes
            .values()
            .filter(|n| matches!(n.state, BranchState::Committed { .. }))
            .max_by(|a, b| {
                a.state.score().unwrap_or(0.0)
                    .partial_cmp(&b.state.score().unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// The best non-pruned branch regardless of commit status (fallback when
    /// no solution found but budget exhausted).
    pub fn best_partial(&self) -> Option<&HypothesisNode> {
        self.nodes
            .values()
            .filter(|n| !matches!(n.state, BranchState::Pruned { .. } | BranchState::Pending))
            .max_by(|a, b| {
                a.state.score().unwrap_or(0.0)
                    .partial_cmp(&b.state.score().unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Count of active branches (pending + simulating + partial).
    pub fn active_count(&self) -> usize {
        self.nodes.values().filter(|n| n.state.is_active()).count()
    }

    /// Total branches in the tree.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Retrieve a node by ID.
    pub fn get(&self, id: BranchId) -> Option<&HypothesisNode> {
        self.nodes.get(&id)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal WorldState implementation for testing.
    #[derive(Clone, Debug)]
    struct Counter {
        value: i32,
        goal: i32,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    enum Increment { Up, Down }

    impl WorldState for Counter {
        type Action = Increment;
        type Observation = i32;
        type Score = OrderedF32;

        fn analyze(&self) -> Vec<Property> {
            vec![
                Property::int("value", self.value as i64),
                Property::bool("at_goal", self.value == self.goal),
            ]
        }

        fn available_actions(&self) -> Vec<Increment> {
            vec![Increment::Up, Increment::Down]
        }

        fn apply(&self, action: &Increment) -> Result<(Self, i32), WorldStateError> {
            let next = Self {
                value: match action {
                    Increment::Up   => self.value + 1,
                    Increment::Down => self.value - 1,
                },
                goal: self.goal,
            };
            let obs = next.value;
            Ok((next, obs))
        }

        fn score_against(&self, goal: &Self) -> OrderedF32 {
            let diff = (self.value - goal.value).unsigned_abs() as f32;
            OrderedF32(1.0 / (1.0 + diff))
        }

        fn diff_to_deltas(&self, _other: &Self) -> Vec<SceneDelta> { vec![] }

        fn to_bytes(&self) -> Result<Vec<u8>, WorldStateError> {
            Ok(vec![self.value as u8])
        }

        fn from_bytes(bytes: &[u8]) -> Result<Self, WorldStateError> {
            Ok(Self { value: bytes[0] as i32, goal: 0 })
        }
    }

    /// Wrapper so f32 can implement PartialOrd + Into<f32>.
    #[derive(Clone, Debug, PartialEq, PartialOrd)]
    struct OrderedF32(f32);

    impl From<OrderedF32> for f32 {
        fn from(v: OrderedF32) -> f32 { v.0 }
    }

    #[test]
    fn sandbox_steps_state() {
        let start = Counter { value: 0, goal: 3 };
        let goal  = Counter { value: 3, goal: 3 };
        let mut sb = Sandbox::new(start, goal);

        sb.apply(Increment::Up).unwrap();
        sb.apply(Increment::Up).unwrap();
        sb.apply(Increment::Up).unwrap();

        assert_eq!(sb.current().value, 3);
        assert!((sb.score_f32() - 1.0).abs() < 1e-6);
        assert_eq!(sb.action_sequence().len(), 3);
    }

    #[test]
    fn sandbox_history_tracks_steps() {
        let start = Counter { value: 0, goal: 0 };
        let goal  = Counter { value: 0, goal: 0 };
        let mut sb = Sandbox::new(start, goal);
        sb.apply(Increment::Down).unwrap();
        assert_eq!(sb.history().len(), 1);
        assert_eq!(sb.step_count, 1);
    }

    #[test]
    fn hypothesis_tree_branch_lifecycle() {
        let mut tree = HypothesisTree::new(SimulationBudget::tight());

        let h = Hypothesis {
            description: "try incrementing".to_string(),
            prior_confidence: 0.8,
            source_law: None,
            motivating_properties: vec![],
        };
        let id = tree.add_branch(h);

        assert!(tree.has_active_branches());
        assert_eq!(tree.next_pending(), Some(id));

        tree.mark_simulating(id);
        tree.score_branch(id, 1.0, 3);

        assert!(matches!(tree.get(id).unwrap().state, BranchState::Committed { .. }));
        assert!(tree.best_committed().is_some());
    }

    #[test]
    fn hypothesis_tree_pruning() {
        let mut tree = HypothesisTree::new(SimulationBudget::default());
        let h = Hypothesis {
            description: "dead end".to_string(),
            prior_confidence: 0.1,
            source_law: None,
            motivating_properties: vec![],
        };
        let id = tree.add_branch(h);
        tree.score_branch(id, 0.01, 1); // below prune_threshold
        assert!(matches!(tree.get(id).unwrap().state, BranchState::Pruned { .. }));
        assert!(!tree.has_active_branches());
    }

    #[test]
    fn property_matching() {
        let p = Property::bool("grid.symmetry.horizontal", true);
        assert!(p.matches("grid.symmetry.horizontal"));
        assert!(p.matches("grid.symmetry"));
        assert!(p.matches("grid"));
        assert!(!p.matches("grid.rotation"));
    }

    #[test]
    fn budget_step_exhaustion() {
        let budget = SimulationBudget { max_total_steps: 5, ..SimulationBudget::default() };
        let mut tree = HypothesisTree::new(budget);
        let id = tree.add_branch(Hypothesis {
            description: "test".to_string(),
            prior_confidence: 0.5,
            source_law: None,
            motivating_properties: vec![],
        });
        tree.score_branch(id, 0.5, 6); // 6 > max_total_steps=5
        assert!(tree.budget_exhausted());
    }
}
