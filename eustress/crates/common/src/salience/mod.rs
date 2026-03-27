//! # Salience Filter — Confidence Lake between Iggy and embedvec
//!
//! ## Table of Contents
//! - SalienceConfig  — weights and thresholds
//! - SalienceScore   — three-factor score for one streaming event
//! - ScoredEvent     — delta + its score, ready for memory tier routing
//! - SalienceFilter  — stateless scorer; call from Bevy systems or async tasks
//!
//! ## Scoring Formula
//!
//! ```text
//! composite = w_goal   × goal_relevance
//!           + w_novelty × novelty
//!           + w_causal  × causal_influence
//! ```
//!
//! - `goal_relevance`   — Jaccard overlap between the delta kind/name and the
//!                        active `GoalTree` description, weighted by the goal's
//!                        declared salience weight.
//! - `novelty`          — `1.0 - cosine_similarity(embedding, nearest_neighbor)`.
//!                        High = far from anything already embedded → worth keeping.
//! - `causal_influence` — estimated fraction of downstream state variables this
//!                        delta touches (populated optional fields / max_fields).

use crate::goals::GoalTree;
use crate::iggy_delta::SceneDelta;
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// SalienceConfig
// ─────────────────────────────────────────────────────────────────────────────

/// Tunable weights and thresholds for the salience scorer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SalienceConfig {
    /// Weight for the goal_relevance term [0.0–1.0].
    pub weight_goal: f32,
    /// Weight for the novelty term [0.0–1.0].
    pub weight_novelty: f32,
    /// Weight for the causal_influence term [0.0–1.0].
    pub weight_causal: f32,
    /// Composite score at or above which an event is promoted to embedvec (Tier 1).
    pub promote_threshold: f32,
    /// Composite score below which an event is dropped immediately (no storage).
    pub drop_threshold: f32,
    /// Assumed maximum populated-field count for causal influence normalisation.
    pub max_fields: f32,
}

impl Default for SalienceConfig {
    fn default() -> Self {
        // Weights sum to 1.0 for interpretable composite scores.
        Self {
            weight_goal: 0.40,
            weight_novelty: 0.35,
            weight_causal: 0.25,
            promote_threshold: 0.45,
            drop_threshold: 0.10,
            max_fields: 8.0,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SalienceScore
// ─────────────────────────────────────────────────────────────────────────────

/// Three-factor salience score for one streaming event.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct SalienceScore {
    /// Relevance to the active goal [0.0–1.0].
    pub goal_relevance: f32,
    /// Distance from the nearest already-embedded event [0.0–1.0].
    /// High = novel; low = redundant.
    pub novelty: f32,
    /// Estimated fraction of downstream state variables this event touches [0.0–1.0].
    pub causal_influence: f32,
    /// Weighted composite — the primary routing signal.
    pub composite: f32,
}

impl SalienceScore {
    /// Whether this score warrants promotion to Tier 1 (embedvec).
    pub fn should_promote(&self, config: &SalienceConfig) -> bool {
        self.composite >= config.promote_threshold
    }

    /// Whether this score warrants immediate drop (no storage at any tier).
    pub fn should_drop(&self, config: &SalienceConfig) -> bool {
        self.composite < config.drop_threshold
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ScoredEvent
// ─────────────────────────────────────────────────────────────────────────────

/// A `SceneDelta` paired with its salience score, ready for the memory tier.
#[derive(Debug, Clone)]
pub struct ScoredEvent {
    /// The original delta.
    pub delta: SceneDelta,
    /// Salience score computed by `SalienceFilter::score`.
    pub score: SalienceScore,
    /// Unix ms timestamp when scoring occurred.
    pub scored_at_ms: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// SalienceFilter
// ─────────────────────────────────────────────────────────────────────────────

/// Stateless salience scorer.
///
/// Wrap in a Bevy `Resource` or store in an `Arc` for async use.
/// It holds no mutable state — call `score()` freely from any context.
///
/// # Example (Bevy system sketch)
/// ```rust,ignore
/// fn salience_system(
///     mut deltas: EventReader<SceneDelta>,
///     goal_tree: Res<GoalTree>,
///     filter:    Res<SalienceFilter>,
///     mut tier:  ResMut<MemoryTierController>,
/// ) {
///     for delta in deltas.read() {
///         let scored = filter.score(delta, &goal_tree, None);
///         tier.route(scored);
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct SalienceFilter {
    pub config: SalienceConfig,
}

impl Default for SalienceFilter {
    fn default() -> Self {
        Self {
            config: SalienceConfig::default(),
        }
    }
}

impl SalienceFilter {
    /// Create with custom config.
    pub fn with_config(config: SalienceConfig) -> Self {
        Self { config }
    }

    /// Score a single `SceneDelta`.
    ///
    /// `nearest_similarity` — cosine similarity to the nearest already-embedded
    /// event (obtain from `EmbedvecIndex::search` on the delta's embedding).
    /// Pass `None` when the index is empty; defaults to 0.0 → maximum novelty.
    pub fn score(
        &self,
        delta: &SceneDelta,
        goal_tree: &GoalTree,
        nearest_similarity: Option<f32>,
    ) -> SalienceScore {
        let cfg = &self.config;

        // ── 1. Goal relevance ────────────────────────────────────────────────
        // Build a short description from the delta kind and name (if present).
        let kind_str = format!("{:?}", delta.kind);
        let name_str = delta
            .name
            .as_ref()
            .map(|n| n.name.as_str())
            .unwrap_or("");
        let combined = format!("{kind_str} {name_str}");

        // Jaccard overlap against active goal description, scaled by the goal's
        // declared salience weight so high-priority goals drive more attention.
        let relevance_raw = goal_tree.goal_relevance(&combined);
        let goal_weight = goal_tree.active_salience_weight().max(0.01);
        let goal_relevance = (relevance_raw * goal_weight).clamp(0.0, 1.0);

        // ── 2. Novelty ───────────────────────────────────────────────────────
        // High similarity to existing embeddings → low novelty (already known).
        let novelty = 1.0 - nearest_similarity.unwrap_or(0.0).clamp(0.0, 1.0);

        // ── 3. Causal influence ──────────────────────────────────────────────
        // Proxy: count how many optional payload fields are populated.
        // A delta that changes transform + name + parent touches more downstream
        // state than one that only changes a single property.
        let populated = [
            delta.transform.is_some(),
            delta.part.is_some(),
            delta.name.is_some(),
            delta.new_parent.is_some(),
        ]
        .iter()
        .filter(|&&b| b)
        .count() as f32;
        let causal_influence = (populated / cfg.max_fields).clamp(0.0, 1.0);

        let composite = (cfg.weight_goal * goal_relevance
            + cfg.weight_novelty * novelty
            + cfg.weight_causal * causal_influence)
            .clamp(0.0, 1.0);

        SalienceScore {
            goal_relevance,
            novelty,
            causal_influence,
            composite,
        }
    }

    /// Score a batch of deltas, dropping those below `drop_threshold`.
    ///
    /// `nearest_similarity` is applied uniformly to all deltas in the batch
    /// (pre-compute per-delta similarity for higher fidelity).
    pub fn score_batch<'a>(
        &self,
        deltas: impl Iterator<Item = &'a SceneDelta>,
        goal_tree: &GoalTree,
        nearest_similarity: Option<f32>,
    ) -> Vec<ScoredEvent> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        deltas
            .filter_map(|delta| {
                let score = self.score(delta, goal_tree, nearest_similarity);
                if score.should_drop(&self.config) {
                    None
                } else {
                    Some(ScoredEvent {
                        delta: delta.clone(),
                        score,
                        scored_at_ms: now_ms,
                    })
                }
            })
            .collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iggy_delta::{SceneDelta, TransformPayload};

    fn transform_delta() -> SceneDelta {
        SceneDelta::transform(
            1,
            1,
            0,
            TransformPayload {
                position: [1.0, 2.0, 3.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0, 1.0, 1.0],
            },
        )
    }

    #[test]
    fn score_in_range() {
        let filter = SalienceFilter::default();
        let tree = GoalTree::default();
        let score = filter.score(&transform_delta(), &tree, None);
        assert!((0.0..=1.0).contains(&score.composite));
        assert!((0.0..=1.0).contains(&score.novelty));
        assert!((0.0..=1.0).contains(&score.goal_relevance));
        assert!((0.0..=1.0).contains(&score.causal_influence));
    }

    #[test]
    fn high_similarity_reduces_novelty() {
        let filter = SalienceFilter::default();
        let tree = GoalTree::default();
        let delta = transform_delta();
        let novel = filter.score(&delta, &tree, Some(0.10));
        let stale = filter.score(&delta, &tree, Some(0.95));
        assert!(novel.novelty > stale.novelty);
    }

    #[test]
    fn fully_stale_fully_novel_drop_check() {
        let filter = SalienceFilter::default();
        let tree = GoalTree::default(); // no active episode
        let delta = transform_delta();

        // No goal active, maximum similarity → only causal_influence contributes.
        // transform has 1 populated field / 8 max → 0.125.
        // weight_causal = 0.25 → composite ≈ 0.03125 < drop_threshold 0.10 → drop.
        let score = filter.score(&delta, &tree, Some(1.0));
        assert!(score.should_drop(&filter.config));

        // With no prior embeddings (max novelty) the transform delta survives.
        let score_novel = filter.score(&delta, &tree, Some(0.0));
        assert!(!score_novel.should_drop(&filter.config));
    }

    #[test]
    fn active_goal_boosts_relevance() {
        let filter = SalienceFilter::default();
        let mut tree = GoalTree::default();
        tree.begin_episode("TransformChanged task", 50);
        let delta = transform_delta(); // kind = "TransformChanged"
        let score_with_goal = filter.score(&delta, &tree, Some(0.5));
        let mut tree_none = GoalTree::default();
        let score_no_goal = filter.score(&delta, &tree_none, Some(0.5));
        assert!(score_with_goal.goal_relevance >= score_no_goal.goal_relevance);
    }
}
