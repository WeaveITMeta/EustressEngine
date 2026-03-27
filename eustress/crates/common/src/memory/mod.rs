//! # Memory Tier Controller — Iggy → embedvec → RocksDB routing
//!
//! ## Table of Contents
//! - MemoryConfig          — TTL, promotion threshold, RocksDB path
//! - TierDecision          — routing decision for one scored event
//! - EpisodeBuffer         — accumulator for end-of-episode archival
//! - MemoryTierController  — routes ScoredEvents to the right storage tier
//!
//! ## Three-Tier Architecture
//!
//! ```text
//! Tier 0: Iggy hot stream   — full fidelity, real-time (managed externally)
//! Tier 1: embedvec warm     — HNSW in-memory, salience-filtered, O(log n) search
//! Tier 2: RocksDB cold      — compressed episode summaries, durable O(log n) lookup
//! Drop:   score < threshold — event discarded, never stored
//! ```
//!
//! The controller does not own the Iggy client or embedvec index. It receives
//! `ScoredEvent`s from `SalienceFilter`, returns a `TierDecision` per event,
//! and the caller acts on the decision (encode into embedvec, write to RocksDB,
//! discard). This keeps the controller dependency-free and testable in isolation.

use crate::salience::{SalienceConfig, ScoredEvent};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

// ─────────────────────────────────────────────────────────────────────────────
// MemoryConfig
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for the memory tier controller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Path to the RocksDB cold-storage database.
    pub rocksdb_path: String,
    /// Composite salience score at or above which an event is promoted to
    /// Tier 1 (embedvec). Should be >= `SalienceConfig::promote_threshold`.
    pub promote_threshold: f32,
    /// Maximum events held in the warm pending buffer before the oldest are
    /// flushed (caller must drain via `drain_warm()`).
    pub warm_buffer_max: usize,
    /// Whether to compress episode summaries before writing to RocksDB.
    pub compress_episodes: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            rocksdb_path: "./eustress_memory".to_string(),
            promote_threshold: 0.45,
            warm_buffer_max: 256,
            compress_episodes: true,
        }
    }
}

impl MemoryConfig {
    pub fn with_rocksdb_path(mut self, path: impl Into<String>) -> Self {
        self.rocksdb_path = path.into();
        self
    }

    pub fn with_promote_threshold(mut self, threshold: f32) -> Self {
        self.promote_threshold = threshold;
        self
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TierDecision
// ─────────────────────────────────────────────────────────────────────────────

/// Routing decision returned by `MemoryTierController::route()`.
#[derive(Debug, Clone, PartialEq)]
pub enum TierDecision {
    /// Promote to Tier 1 (embedvec warm). Caller encodes and inserts.
    Promote,
    /// Hold in the warm pending buffer; promote when `drain_warm()` is called.
    Buffer,
    /// Score too low — event is not stored anywhere.
    Drop,
    /// Compress and write to Tier 2 (RocksDB cold). Used at episode end.
    Archive,
}

// ─────────────────────────────────────────────────────────────────────────────
// EpisodeBuffer
// ─────────────────────────────────────────────────────────────────────────────

/// Accumulates promoted events during an episode for end-of-episode compression
/// and archival to RocksDB (Tier 2).
#[derive(Debug, Default)]
pub struct EpisodeBuffer {
    /// Promoted events in arrival order.
    pub events: VecDeque<ScoredEvent>,
    /// Total events seen (including dropped) — used for promotion_rate.
    pub total_seen: u64,
    /// Total events added to this buffer.
    pub total_promoted: u64,
    /// Unix ms timestamp when this episode started.
    pub started_at_ms: u64,
}

impl EpisodeBuffer {
    pub fn new(started_at_ms: u64) -> Self {
        Self {
            started_at_ms,
            ..Default::default()
        }
    }

    /// Push a promoted event.
    pub fn push(&mut self, event: ScoredEvent) {
        self.events.push_back(event);
        self.total_promoted += 1;
    }

    /// Fraction of seen events that were promoted [0.0–1.0].
    pub fn promotion_rate(&self) -> f32 {
        if self.total_seen == 0 {
            return 0.0;
        }
        self.total_promoted as f32 / self.total_seen as f32
    }

    /// Average composite salience of promoted events.
    pub fn mean_salience(&self) -> f32 {
        if self.events.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.events.iter().map(|e| e.score.composite).sum();
        sum / self.events.len() as f32
    }

    /// Reset for the next episode.
    pub fn reset(&mut self, started_at_ms: u64) {
        self.events.clear();
        self.total_seen = 0;
        self.total_promoted = 0;
        self.started_at_ms = started_at_ms;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MemoryTierController
// ─────────────────────────────────────────────────────────────────────────────

/// Routes `ScoredEvent`s from the salience filter to the right memory tier.
///
/// Not a Bevy Resource intentionally — wrap in `Arc<Mutex<>>` when sharing
/// across Bevy systems and async Iggy tasks. This keeps it compatible with
/// both ECS and async contexts without Bevy coupling.
#[derive(Debug)]
pub struct MemoryTierController {
    pub config: MemoryConfig,
    salience_config: SalienceConfig,
    /// Current episode buffer (None between episodes).
    pub episode_buffer: Option<EpisodeBuffer>,
    /// Warm-tier events pending flush to embedvec.
    pub warm_pending: VecDeque<ScoredEvent>,
}

impl MemoryTierController {
    /// Create with explicit configs.
    pub fn new(config: MemoryConfig, salience_config: SalienceConfig) -> Self {
        Self {
            config,
            salience_config,
            episode_buffer: None,
            warm_pending: VecDeque::new(),
        }
    }

    /// Signal episode start — initialises the episode buffer.
    pub fn begin_episode(&mut self, started_at_ms: u64) {
        self.episode_buffer = Some(EpisodeBuffer::new(started_at_ms));
    }

    /// Route a single scored event. Returns the decision for the caller to act on.
    ///
    /// The caller should:
    /// - `Promote`  → encode event into embedvec index
    /// - `Buffer`   → call `drain_warm()` on the next cycle
    /// - `Drop`     → discard
    /// - `Archive`  → should not appear from `route()`; only from `end_episode()`
    pub fn route(&mut self, event: ScoredEvent) -> TierDecision {
        if let Some(buf) = &mut self.episode_buffer {
            buf.total_seen += 1;
        }

        if event.score.should_drop(&self.salience_config) {
            return TierDecision::Drop;
        }

        if event.score.composite >= self.config.promote_threshold {
            if let Some(buf) = &mut self.episode_buffer {
                buf.push(event.clone());
            }
            if self.warm_pending.len() < self.config.warm_buffer_max {
                self.warm_pending.push_back(event);
                TierDecision::Promote
            } else {
                // Buffer full — caller must drain before the next promote cycle.
                TierDecision::Buffer
            }
        } else {
            TierDecision::Buffer
        }
    }

    /// Drain all pending warm-tier events for encoding into embedvec.
    pub fn drain_warm(&mut self) -> Vec<ScoredEvent> {
        self.warm_pending.drain(..).collect()
    }

    /// Signal episode end. Returns the accumulated `EpisodeBuffer` for RocksDB
    /// archival and resets state for the next episode.
    ///
    /// Caller is responsible for:
    /// 1. Serialising the buffer into an `ArcEpisodeRecord`.
    /// 2. Publishing to the `arc_episodes` Iggy topic.
    /// 3. Writing the compressed summary to RocksDB Tier 2.
    pub fn end_episode(&mut self, ended_at_ms: u64) -> Option<EpisodeBuffer> {
        let buf = self.episode_buffer.take();
        tracing::info!(
            promotion_rate = buf.as_ref().map(|b| b.promotion_rate()).unwrap_or(0.0),
            mean_salience  = buf.as_ref().map(|b| b.mean_salience()).unwrap_or(0.0),
            duration_ms    = buf.as_ref().map(|b| ended_at_ms.saturating_sub(b.started_at_ms)).unwrap_or(0),
            "MemoryTierController: episode ended"
        );
        buf
    }

    /// Whether an episode is currently active.
    pub fn in_episode(&self) -> bool {
        self.episode_buffer.is_some()
    }

    /// Number of events pending in the warm buffer.
    pub fn warm_pending_count(&self) -> usize {
        self.warm_pending.len()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iggy_delta::{SceneDelta, TransformPayload};
    use crate::salience::SalienceScore;

    fn make_scored(composite: f32) -> ScoredEvent {
        ScoredEvent {
            delta: SceneDelta::transform(
                1,
                1,
                0,
                TransformPayload {
                    position: [0.0; 3],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                },
            ),
            score: SalienceScore {
                goal_relevance: composite,
                novelty: composite,
                causal_influence: composite,
                composite,
            },
            scored_at_ms: 1_000,
        }
    }

    fn default_ctrl() -> MemoryTierController {
        MemoryTierController::new(MemoryConfig::default(), SalienceConfig::default())
    }

    #[test]
    fn promote_and_drop() {
        let mut ctrl = default_ctrl();
        ctrl.begin_episode(0);

        assert_eq!(ctrl.route(make_scored(0.80)), TierDecision::Promote);
        assert_eq!(ctrl.route(make_scored(0.05)), TierDecision::Drop);

        let warm = ctrl.drain_warm();
        assert_eq!(warm.len(), 1);
    }

    #[test]
    fn episode_lifecycle() {
        let mut ctrl = default_ctrl();
        assert!(!ctrl.in_episode());
        ctrl.begin_episode(0);
        assert!(ctrl.in_episode());
        ctrl.route(make_scored(0.80));
        let buf = ctrl.end_episode(5_000).unwrap();
        assert_eq!(buf.total_promoted, 1);
        assert_eq!(buf.total_seen, 1);
        assert!(!ctrl.in_episode());
    }

    #[test]
    fn warm_buffer_cap() {
        let config = MemoryConfig {
            warm_buffer_max: 2,
            ..Default::default()
        };
        let mut ctrl = MemoryTierController::new(config, SalienceConfig::default());
        ctrl.begin_episode(0);

        // First two → Promote; third → Buffer (cap reached)
        assert_eq!(ctrl.route(make_scored(0.80)), TierDecision::Promote);
        assert_eq!(ctrl.route(make_scored(0.80)), TierDecision::Promote);
        assert_eq!(ctrl.route(make_scored(0.80)), TierDecision::Buffer);
        assert_eq!(ctrl.warm_pending_count(), 2);
    }

    #[test]
    fn promotion_rate() {
        let mut ctrl = default_ctrl();
        ctrl.begin_episode(0);
        ctrl.route(make_scored(0.80)); // promote
        ctrl.route(make_scored(0.05)); // drop
        ctrl.route(make_scored(0.30)); // buffer
        let buf = ctrl.end_episode(1_000).unwrap();
        // total_seen = 3 (all three increment it)
        assert_eq!(buf.total_seen, 3);
        // total_promoted = 1 (only the 0.80 event crossed promote_threshold)
        assert_eq!(buf.total_promoted, 1);
        assert!((buf.promotion_rate() - 1.0 / 3.0).abs() < 1e-5);
    }
}
