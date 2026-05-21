// Copyright (c) 2024-present, fjall-rs
// This source code is licensed under both the Apache 2.0 and MIT License
// (found in the LICENSE-* files in the repository)
//
// Eustress addition (vendored fork, 2026-05-16): the 3D-locality-
// preserving compaction deep item. Implemented against the real
// upstream `CompactionStrategy` trait, in the SAME safety envelope as
// the built-in `maintenance` strategy: it NEVER overrides a decision
// the inner strategy made (so leveled/tiered correctness invariants
// are untouched) — it only acts in the idle gap (inner said
// `DoNothing`) and only proposes a bounded merge of key-CONTIGUOUS
// segments. Under Morton/Z-order-encoded keys, byte-key adjacency IS
// 3D spatial adjacency, so co-locating key-adjacent segments keeps
// spatially-near entities in the same SSTable — the locality goal —
// without any change to compaction's correctness-critical selection.

use super::{Choice, CompactionStrategy};
use crate::{
    config::Config, level_manifest::LevelManifest, segment::meta::SegmentId, segment::Segment,
};
use std::sync::Arc;

/// 3D-locality-preserving compaction wrapper (Eustress fork addition).
///
/// Wraps any inner [`CompactionStrategy`]. If the inner strategy wants
/// to do anything (`Move`/`Merge`/`Drop`) that decision is returned
/// untouched — its correctness invariants are never second-guessed.
/// Only when the inner strategy is idle (`DoNothing`) and level 0 has
/// grown past `l0_soft_cap` does this strategy propose its own merge:
/// the `merge_width` segments that are most contiguous in key space.
/// Merging a key-contiguous run is always a valid level-0 maintenance
/// compaction (the built-in `maintenance` strategy merges arbitrary
/// windows), so this is the same safety class — purely a
/// which-segments heuristic that improves spatial read locality.
#[derive(Clone)]
pub struct Strategy {
    inner: Arc<dyn CompactionStrategy + Send + Sync>,
    /// Only act once level 0 exceeds this many segments (matches the
    /// built-in maintenance strategy's idle-cleanup philosophy).
    l0_soft_cap: usize,
    /// How many key-adjacent segments to co-locate per idle merge.
    merge_width: usize,
}

impl Strategy {
    /// Wrap `inner`. `l0_soft_cap` gates idle action; `merge_width`
    /// bounds the merge (both clamped to at least 2 / 1).
    #[must_use]
    pub fn new(
        inner: Arc<dyn CompactionStrategy + Send + Sync>,
        l0_soft_cap: usize,
        merge_width: usize,
    ) -> Self {
        Self {
            inner,
            l0_soft_cap: l0_soft_cap.max(2),
            merge_width: merge_width.max(2),
        }
    }
}

impl CompactionStrategy for Strategy {
    fn get_name(&self) -> &'static str {
        "EustressLocality"
    }

    fn choose(&self, levels: &LevelManifest, config: &Config) -> Choice {
        // Never override the inner strategy's real decisions — only
        // fill its idle gaps. This is what keeps correctness invariants
        // (leveled level-targeting, tiered run formation) entirely the
        // inner strategy's responsibility.
        match self.inner.choose(levels, config) {
            Choice::DoNothing => {}
            decided => return decided,
        }

        let resolved = levels.resolved_view();
        let Some(l0) = resolved.first() else {
            return Choice::DoNothing;
        };
        if l0.len() <= self.l0_soft_cap {
            return Choice::DoNothing;
        }

        // Sort a clone of level 0 by minimum key. A contiguous window
        // in this order is contiguous in key space, hence (for
        // Morton-encoded keys) spatially adjacent. `UserKey: Ord`.
        let mut lvl = l0.clone();
        lvl.segments
            .sort_by(|a, b| a.metadata.key_range.min().cmp(b.metadata.key_range.min()));

        let n = self.merge_width.min(lvl.segments.len());
        if n < 2 {
            return Choice::DoNothing;
        }
        let segment_ids: crate::HashSet<SegmentId> =
            lvl.segments[..n].iter().map(Segment::id).collect();

        Choice::Merge(super::Input {
            dest_level: 0,
            segment_ids,
            target_size: u64::MAX,
        })
    }
}
