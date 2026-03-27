//! # Simulation Records — Iggy Stream Payload Types
//!
//! ## Table of Contents
//! - SimRecord              — one Monte Carlo run_simulation() result (replaces bincode file cache)
//! - IterationRecord        — one VIGA / workshop feedback cycle iteration
//! - RuneScriptRecord       — one Rune script execute_and_apply() result
//! - WorkshopIterationRecord — aggregate of one full optimize→simulate→Rune cycle
//! - BranchPosterior        — per-branch probability snapshot inside SimRecord
//! - CodeDiffRecord         — minimal diff summary between two Rune code versions
//!
//! ## Design
//!
//! All types use rkyv 0.8 for **zero-copy binary serialization** onto Iggy topics.
//! serde derives are included for TOML/JSON debug output and CLI replay.
//!
//! The **Iggy append-only log** replaces file-system caching:
//!
//! ```text
//! Old (removed):   run_simulation() → zstd+bincode → .escn file
//! New:             run_simulation() → rkyv → Iggy topic "eustress/sim_results"
//!                                          → poll to replay any past run
//!                                          → CLI: eustress sim replay --run <id>
//!
//! Old (removed):   VigaContext.history: VecDeque<IterationHistory> (in-memory, lost on restart)
//! New:             process_feedback() → rkyv → Iggy topic "eustress/iteration_history"
//!                                             → best() query for highest similarity run
//!                                             → resume from any iteration seq
//!
//! Old (removed):   execute_and_apply() logs only in memory
//! New:             execute_and_apply() → rkyv → Iggy topic "eustress/rune_scripts"
//!                                              → full audit trail of every Rune execution
//!                                              → replayable for deterministic replay
//! ```
//!
//! ## Feature Gate
//! Always compiled — rkyv is a non-optional workspace dependency.
//! Iggy publishing is done via `sim_stream.rs` (feature-gated by `iggy-streaming`).

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// BranchPosterior — compact branch probability entry inside SimRecord
// ─────────────────────────────────────────────────────────────────────────────

/// Compact per-branch posterior + hit count snapshot from one simulation run.
/// UUID stored as (hi, lo) u64 pair for rkyv compatibility.
#[derive(
    Archive, RkyvSerialize, RkyvDeserialize,
    Serialize, Deserialize,
    Debug, Clone, PartialEq,
)]
#[rkyv(derive(Debug, PartialEq))]
pub struct BranchPosterior {
    /// Branch UUID high 64 bits.
    pub uuid_hi: u64,
    /// Branch UUID low 64 bits.
    pub uuid_lo: u64,
    /// Branch label (truncated to 128 bytes if longer).
    pub label: String,
    /// Posterior probability after Bayesian update + MC blend.
    pub posterior: f64,
    /// Monte Carlo hit count.
    pub mc_hits: u64,
    /// Whether this is a leaf node (terminal branch).
    pub is_leaf: bool,
    /// Whether this branch was soft-pruned (collapsed) by this run.
    pub is_pruned: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// SimRecord — replaces .escn file cache
// ─────────────────────────────────────────────────────────────────────────────

/// Full output of one `run_simulation()` call.
///
/// Published to Iggy topic `eustress/sim_results`.
/// Replaces the removed `ScenarioPersistence` file-based cache.
///
/// Consumers can:
/// - Poll the topic to get all past runs in order.
/// - Filter by `scenario_id` to get runs for a specific scenario.
/// - Compare `posteriors` across runs to see how the tree converged.
/// - Resume from the last run's posteriors as priors for the next run.
#[derive(
    Archive, RkyvSerialize, RkyvDeserialize,
    Serialize, Deserialize,
    Debug, Clone, PartialEq,
)]
#[rkyv(derive(Debug, PartialEq))]
pub struct SimRecord {
    /// Unique run identifier (UUID v4 as u128).
    pub run_id: u128,

    /// The scenario this run belongs to (UUID v4 as u128).
    pub scenario_id: u128,

    /// Human-readable scenario name (snapshot at time of run).
    pub scenario_name: String,

    /// Random seed used (0 = non-deterministic run).
    pub seed: u64,

    /// Total Monte Carlo samples executed.
    pub total_samples: u64,

    /// Per-branch posteriors (all branches, not just leaves).
    pub posteriors: Vec<BranchPosterior>,

    /// Wall-clock duration of the simulation in milliseconds.
    pub duration_ms: u64,

    /// Unix timestamp in ms when this run completed.
    pub completed_at_ms: u64,

    /// Whether Bayesian updates were applied before sampling.
    pub applied_bayesian: bool,

    /// Whether soft-pruning was applied after sampling.
    pub applied_soft_prune: bool,

    /// Monotonic sequence number within this session (for ordering runs).
    pub session_seq: u64,
}

impl SimRecord {
    /// Serialize to rkyv bytes for Iggy publishing.
    pub fn to_bytes(&self) -> Result<Vec<u8>, rkyv::rancor::Error> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self).map(|b| b.to_vec())
    }

    /// Deserialize from rkyv bytes polled from Iggy.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, rkyv::rancor::Error> {
        rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes)
    }

    /// Find the branch with highest posterior probability.
    pub fn best_branch(&self) -> Option<&BranchPosterior> {
        self.posteriors.iter().filter(|b| b.is_leaf).max_by(|a, b| {
            a.posterior.partial_cmp(&b.posterior).unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    /// Total number of active (non-pruned) leaf branches.
    pub fn active_leaf_count(&self) -> usize {
        self.posteriors.iter().filter(|b| b.is_leaf && !b.is_pruned).count()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// CodeDiffRecord — minimal diff between two Rune script versions
// ─────────────────────────────────────────────────────────────────────────────

/// Minimal summary of what changed between two Rune code versions.
/// Stored inline in IterationRecord to avoid large payloads.
#[derive(
    Archive, RkyvSerialize, RkyvDeserialize,
    Serialize, Deserialize,
    Debug, Clone, PartialEq, Default,
)]
#[rkyv(derive(Debug, PartialEq))]
pub struct CodeDiffRecord {
    /// Lines added relative to the previous iteration.
    pub lines_added: u32,
    /// Lines removed relative to the previous iteration.
    pub lines_removed: u32,
    /// One-line summary of the most significant change.
    pub summary: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// IterationRecord — replaces in-memory VigaContext.history
// ─────────────────────────────────────────────────────────────────────────────

/// One complete feedback cycle from the VIGA generator-verifier loop or
/// any iterative workshop optimization loop.
///
/// Published to Iggy topic `eustress/iteration_history`.
/// Replaces the removed `VigaContext.history: VecDeque<IterationHistory>`.
///
/// Enables:
/// - Resuming a multi-session VIGA run from the last best iteration.
/// - Querying the best similarity score achieved across all sessions.
/// - Building a convergence curve for the Studio UI.
/// - Cross-run analysis: which Rune patterns converge fastest?
#[derive(
    Archive, RkyvSerialize, RkyvDeserialize,
    Serialize, Deserialize,
    Debug, Clone, PartialEq,
)]
#[rkyv(derive(Debug, PartialEq))]
pub struct IterationRecord {
    /// Unique iteration run ID (UUID v4 as u128).
    pub run_id: u128,

    /// Session this iteration belongs to.
    pub session_id: u128,

    /// Zero-based iteration number within this session.
    pub iteration: u32,

    /// The Rune code generated by the LLM for this iteration.
    pub generated_code: String,

    /// Similarity score from the verifier (0.0 – 1.0).
    pub similarity: f32,

    /// Whether this is the best similarity achieved so far in this session.
    pub is_best: bool,

    /// Feedback text from the verifier LLM.
    pub verifier_feedback: String,

    /// Diff from the previous iteration's code.
    pub code_diff: CodeDiffRecord,

    /// Wall-clock duration of this iteration in milliseconds.
    pub duration_ms: u64,

    /// Unix timestamp in ms when this iteration completed.
    pub completed_at_ms: u64,

    /// Reference image hash (first 16 bytes of blake3, hex-encoded) — identifies the task.
    pub reference_hash: String,

    /// Optional base64-encoded screenshot thumbnail (~4KB JPEG at quality 30).
    /// Empty string when screenshots are not captured.
    pub screenshot_thumb: String,
}

impl IterationRecord {
    /// Serialize to rkyv bytes for Iggy publishing.
    pub fn to_bytes(&self) -> Result<Vec<u8>, rkyv::rancor::Error> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self).map(|b| b.to_vec())
    }

    /// Deserialize from rkyv bytes polled from Iggy.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, rkyv::rancor::Error> {
        rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// RuneScriptRecord — audit trail for every Rune execution
// ─────────────────────────────────────────────────────────────────────────────

/// Record of one Rune script `execute_and_apply()` call against a Scenario.
///
/// Published to Iggy topic `eustress/rune_scripts`.
///
/// Enables:
/// - Full audit trail: which scripts changed which branches, when.
/// - Deterministic replay: re-apply all recorded scripts to reconstruct scenario state.
/// - AI feedback loop: feed script results back to Claude to improve next generation.
#[derive(
    Archive, RkyvSerialize, RkyvDeserialize,
    Serialize, Deserialize,
    Debug, Clone, PartialEq,
)]
#[rkyv(derive(Debug, PartialEq))]
pub struct RuneScriptRecord {
    /// Unique record ID (UUID v4 as u128).
    pub record_id: u128,

    /// The scenario this script ran against.
    pub scenario_id: u128,

    /// The branch this script was attached to / executed in context of.
    pub branch_id: u128,

    /// Full Rune source code that was executed.
    pub source: String,

    /// Whether the script compiled and executed successfully.
    pub success: bool,

    /// Error message if success == false.
    pub error: String,

    /// Log messages emitted by `scenario::log()` calls.
    pub log_messages: Vec<String>,

    /// Branch labels that had their probability changed by this script.
    pub probability_overrides: Vec<(String, f64)>,

    /// Branch labels that were collapsed by this script.
    pub collapsed_branches: Vec<String>,

    /// New branches created by `scenario::add_branch()`.
    pub new_branches: Vec<(String, String, f64)>, // (parent_label, new_label, probability)

    /// Wall-clock execution time in microseconds.
    pub execution_us: u64,

    /// Unix timestamp in ms when this script ran.
    pub executed_at_ms: u64,

    /// Session sequence number for ordering.
    pub session_seq: u64,
}

impl RuneScriptRecord {
    /// Serialize to rkyv bytes for Iggy publishing.
    pub fn to_bytes(&self) -> Result<Vec<u8>, rkyv::rancor::Error> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self).map(|b| b.to_vec())
    }

    /// Deserialize from rkyv bytes polled from Iggy.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, rkyv::rancor::Error> {
        rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// WorkshopIterationRecord — aggregate of one full workshop optimize cycle
// ─────────────────────────────────────────────────────────────────────────────

/// Aggregate record of one complete workshop product-optimization iteration:
///   properties snapshot → run_simulation() → Rune script → result delta
///
/// Published to Iggy topic `eustress/workshop_iterations`.
///
/// This is the primary record the workshop convergence analysis reads to
/// understand how product iterations are improving across cycles.
#[derive(
    Archive, RkyvSerialize, RkyvDeserialize,
    Serialize, Deserialize,
    Debug, Clone, PartialEq,
)]
#[rkyv(derive(Debug, PartialEq))]
pub struct WorkshopIterationRecord {
    /// Unique workshop iteration ID (UUID v4 as u128).
    pub iteration_id: u128,

    /// Product / workspace identifier this iteration belongs to.
    pub product_id: u128,

    /// Human-readable product name snapshot.
    pub product_name: String,

    /// Zero-based generation number across all sessions for this product.
    pub generation: u32,

    /// The SimRecord.run_id this iteration's simulation result maps to.
    /// Use this to join against the `sim_results` topic for full posteriors.
    pub sim_run_id: u128,

    /// Best leaf branch label from the simulation (the predicted optimal path).
    pub best_branch_label: String,

    /// Best leaf branch posterior probability.
    pub best_branch_posterior: f64,

    /// Fitness score computed after applying Rune scripts (domain-specific, 0.0–1.0).
    /// E.g. weighted sum of: posterior × similarity × structural_integrity_score.
    pub fitness: f64,

    /// Whether this generation beat the previous best fitness.
    pub is_best_generation: bool,

    /// The Rune script source that produced this generation's mutations.
    /// Empty string if no script was run this cycle.
    pub rune_source: String,

    /// Rune script log output for this generation.
    pub rune_logs: Vec<String>,

    /// Key properties snapshot at the start of this generation (TOML fragment).
    /// Captured from the Bevy World before the simulation runs.
    /// Kept compact: max 4KB, truncated if longer.
    pub properties_snapshot_toml: String,

    /// Wall-clock duration of this full cycle in milliseconds.
    pub cycle_duration_ms: u64,

    /// Unix timestamp in ms when this generation completed.
    pub completed_at_ms: u64,
}

impl WorkshopIterationRecord {
    /// Serialize to rkyv bytes for Iggy publishing.
    pub fn to_bytes(&self) -> Result<Vec<u8>, rkyv::rancor::Error> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self).map(|b| b.to_vec())
    }

    /// Deserialize from rkyv bytes polled from Iggy.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, rkyv::rancor::Error> {
        rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ArcEpisodeRecord — one complete ARC-AGI-3 / gym-style episode
// ─────────────────────────────────────────────────────────────────────────────

/// One complete ARC-AGI-3 (or any gym-style) episode.
/// Published to Iggy topic `eustress/arc_episodes`.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[rkyv(derive(Debug, PartialEq))]
pub struct ArcEpisodeRecord {
    /// Unique episode identifier (UUID v4 as u128).
    pub episode_id: u128,
    /// ARC task identifier e.g. "ls20".
    pub task_id: String,
    /// Steps taken in this episode.
    pub total_steps: u32,
    /// Whether the goal was reached.
    pub goal_reached: bool,
    /// Final score 0.0–1.0 per ARC scorecard.
    pub final_score: f32,
    /// Human baseline step count from ARC benchmark metadata.
    pub human_baseline_steps: u32,
    /// total_steps / human_baseline_steps (lower = better).
    pub efficiency_ratio: f32,
    /// Ordered list of action labels taken each step.
    pub actions_taken: Vec<String>,
    /// JSON observation snapshots per step (one per step).
    pub observations: Vec<String>,
    /// Wall-clock duration of this episode in milliseconds.
    pub duration_ms: u64,
    /// Unix timestamp in ms when this episode completed.
    pub completed_at_ms: u64,
    /// Session this episode belongs to.
    pub session_id: u128,
}

impl ArcEpisodeRecord {
    /// Serialize to rkyv bytes for Iggy publishing.
    pub fn to_bytes(&self) -> Result<Vec<u8>, rkyv::rancor::Error> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self).map(|b| b.to_vec())
    }

    /// Deserialize from rkyv bytes polled from Iggy.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, rkyv::rancor::Error> {
        rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sim_record() -> SimRecord {
        SimRecord {
            run_id: 0xdeadbeef_cafebabe_0000_0001,
            scenario_id: 0xdeadbeef_cafebabe_0000_0002,
            scenario_name: "Supply Chain Risk Q1".to_string(),
            seed: 42,
            total_samples: 10_000,
            posteriors: vec![
                BranchPosterior {
                    uuid_hi: 1, uuid_lo: 0,
                    label: "Disruption".to_string(),
                    posterior: 0.72,
                    mc_hits: 7200,
                    is_leaf: true,
                    is_pruned: false,
                },
                BranchPosterior {
                    uuid_hi: 2, uuid_lo: 0,
                    label: "No Disruption".to_string(),
                    posterior: 0.28,
                    mc_hits: 2800,
                    is_leaf: true,
                    is_pruned: false,
                },
            ],
            duration_ms: 48,
            completed_at_ms: 1_700_000_000_000,
            applied_bayesian: true,
            applied_soft_prune: true,
            session_seq: 1,
        }
    }

    #[test]
    fn sim_record_round_trip() {
        let r = make_sim_record();
        let bytes = r.to_bytes().expect("serialize");
        let back = SimRecord::from_bytes(&bytes).expect("deserialize");
        assert_eq!(r, back);
    }

    #[test]
    fn sim_record_best_branch() {
        let r = make_sim_record();
        let best = r.best_branch().expect("has a leaf");
        assert_eq!(best.label, "Disruption");
        assert!((best.posterior - 0.72).abs() < 1e-9);
    }

    #[test]
    fn iteration_record_round_trip() {
        let r = IterationRecord {
            run_id: 1,
            session_id: 2,
            iteration: 3,
            generated_code: "set_probability(\"A\", 0.9)".to_string(),
            similarity: 0.87,
            is_best: true,
            verifier_feedback: "Good spatial layout, improve color.".to_string(),
            code_diff: CodeDiffRecord { lines_added: 2, lines_removed: 1, summary: "+color fix".to_string() },
            duration_ms: 1200,
            completed_at_ms: 1_700_000_001_000,
            reference_hash: "abc123".to_string(),
            screenshot_thumb: String::new(),
        };
        let bytes = r.to_bytes().expect("serialize");
        let back = IterationRecord::from_bytes(&bytes).expect("deserialize");
        assert_eq!(r, back);
    }

    #[test]
    fn rune_script_record_round_trip() {
        let r = RuneScriptRecord {
            record_id: 99,
            scenario_id: 1,
            branch_id: 2,
            source: "set_probability(\"X\", 0.5)".to_string(),
            success: true,
            error: String::new(),
            log_messages: vec!["applied X=0.5".to_string()],
            probability_overrides: vec![("X".to_string(), 0.5)],
            collapsed_branches: vec![],
            new_branches: vec![],
            execution_us: 320,
            executed_at_ms: 1_700_000_002_000,
            session_seq: 5,
        };
        let bytes = r.to_bytes().expect("serialize");
        let back = RuneScriptRecord::from_bytes(&bytes).expect("deserialize");
        assert_eq!(r, back);
    }

    #[test]
    fn arc_episode_record_round_trip() {
        let r = ArcEpisodeRecord {
            episode_id: 0xabcd_1234_5678_9abc_def0_1234_5678_9abc,
            task_id: "ls20".to_string(),
            total_steps: 12,
            goal_reached: true,
            final_score: 0.95,
            human_baseline_steps: 10,
            efficiency_ratio: 1.2,
            actions_taken: vec!["ACTION1".to_string(), "PLACE_0_0".to_string()],
            observations: vec!["{\"grid\":[]}".to_string()],
            duration_ms: 4200,
            completed_at_ms: 1_700_000_003_000,
            session_id: 7,
        };
        let bytes = r.to_bytes().expect("serialize");
        let back = ArcEpisodeRecord::from_bytes(&bytes).expect("deserialize");
        assert_eq!(r, back);
    }
}
