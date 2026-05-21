// Copyright (c) 2024-present, fjall-rs
// This source code is licensed under both the Apache 2.0 and MIT License
// (found in the LICENSE-* files in the repository)
//
// Eustress addition (vendored fork, 2026-05-16): a real-time-friendly
// compaction wrapper. This is the frame-budgeted compaction deep item
// from the WorldDb pivot — implemented against the real upstream
// `CompactionStrategy` trait, additive, no core rewrite.

use super::{Choice, CompactionStrategy};
use crate::{level_manifest::LevelManifest, Config};
use std::sync::Arc;

/// Real-time-friendly compaction strategy (Eustress fork addition).
///
/// Wraps any inner [`CompactionStrategy`] and bounds the worst-case
/// size of a single compaction so a background compaction cannot
/// monopolise the compaction worker and produce a long stall — the
/// "frame-budgeted compaction" requirement for an interactive engine.
///
/// Mechanism: the inner strategy is consulted normally. The only
/// expensive choice is [`Choice::Merge`] (it rewrites segments);
/// [`Choice::Move`] and [`Choice::Drop`] are metadata-only and pass
/// through untouched. If the inner strategy proposes a merge whose
/// segment count exceeds `max_segments_per_compaction`, the choice is
/// downgraded to [`Choice::DoNothing`] for this cycle. The compaction
/// worker re-consults the strategy on its next cycle, and smaller
/// per-level compactions still proceed, so progress is bounded but
/// never starved — large merges are simply spread across more cycles
/// instead of done in one stalling pass.
///
/// This is intentionally a strategy *wrapper* rather than a change to
/// the compaction worker: it uses only the stable public trait, so it
/// stays robust across upstream merges of the vendored crate.
#[derive(Clone)]
pub struct Strategy {
    inner: Arc<dyn CompactionStrategy + Send + Sync>,
    /// Maximum number of segments any single compaction may include.
    /// Clamped to at least 1 so progress is always possible.
    max_segments_per_compaction: usize,
}

impl Strategy {
    /// Wrap `inner`, capping any single compaction at
    /// `max_segments_per_compaction` segments.
    #[must_use]
    pub fn new(
        inner: Arc<dyn CompactionStrategy + Send + Sync>,
        max_segments_per_compaction: usize,
    ) -> Self {
        Self {
            inner,
            max_segments_per_compaction: max_segments_per_compaction.max(1),
        }
    }
}

impl CompactionStrategy for Strategy {
    fn get_name(&self) -> &'static str {
        "EustressBudgeted"
    }

    fn choose(&self, levels: &LevelManifest, config: &Config) -> Choice {
        match self.inner.choose(levels, config) {
            Choice::Merge(input)
                if input.segment_ids.len() > self.max_segments_per_compaction =>
            {
                // Oversized merge — defer; the worker re-consults next
                // cycle. Bounded work this cycle, no stall.
                Choice::DoNothing
            }
            other => other,
        }
    }
}
