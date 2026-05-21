// Copyright (c) 2024-present, fjall-rs
// This source code is licensed under both the Apache 2.0 and MIT License
// (found in the LICENSE-* files in the repository)

pub(crate) mod manager;
pub(crate) mod worker;

use std::sync::Arc;

pub use lsm_tree::compaction::{Fifo, Leveled, Levelled, SizeTiered};

/// Compaction strategy
#[derive(Clone)]
#[allow(clippy::module_name_repetitions)]
pub enum Strategy {
    /// Leveled compaction
    Leveled(crate::compaction::Leveled),

    /// Size-tiered compaction
    SizeTiered(crate::compaction::SizeTiered),

    /// FIFO compaction
    Fifo(crate::compaction::Fifo),
}

impl std::fmt::Debug for Strategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::SizeTiered(_) => "SizeTieredStrategy",
                Self::Leveled(_) => "LeveledStrategy",
                Self::Fifo(_) => "FifoStrategy",
            }
        )
    }
}

impl Default for Strategy {
    fn default() -> Self {
        Self::Leveled(crate::compaction::Leveled::default())
    }
}

/// Eustress fork (2026-05-16): maximum segments any single background
/// compaction may rewrite in one cycle. The frame-budgeted wrapper
/// defers a larger merge to the next cycle so an interactive engine
/// never takes a long compaction stall. Bounded merge width is the
/// "real-time / frame-budgeted compaction" deep item — applied to
/// EVERY partition here so no per-partition wiring or options-format
/// change is needed, and it composes with whichever base strategy
/// (Leveled / SizeTiered / Fifo) the partition selected.
const EUSTRESS_MAX_SEGMENTS_PER_COMPACTION: usize = 8;
/// Level-0 size past which the locality wrapper performs an idle,
/// key-adjacent maintenance merge.
const EUSTRESS_LOCALITY_L0_SOFT_CAP: usize = 12;
/// Segments co-located per idle locality merge (≤ the budget cap so
/// the budgeted wrapper never has to defer a locality merge).
const EUSTRESS_LOCALITY_MERGE_WIDTH: usize = 4;

impl Strategy {
    pub(crate) fn inner(&self) -> Arc<dyn lsm_tree::compaction::CompactionStrategy + Send + Sync> {
        let base: Arc<dyn lsm_tree::compaction::CompactionStrategy + Send + Sync> = match self {
            Self::Leveled(s) => Arc::new(s.clone()),
            Self::SizeTiered(s) => Arc::new(s.clone()),
            Self::Fifo(s) => Arc::new(s.clone()),
        };
        // Compose, innermost → outermost:
        //   base → Locality (adds key-adjacent idle merges, never
        //          overrides base's decisions) → Budgeted (caps any
        //          single merge so a background compaction can't stall
        //          the interactive engine).
        // Every fjall partition gets both, with no new serialized
        // enum variant and no options-format migration.
        let local = Arc::new(lsm_tree::compaction::Locality::new(
            base,
            EUSTRESS_LOCALITY_L0_SOFT_CAP,
            EUSTRESS_LOCALITY_MERGE_WIDTH,
        ));
        Arc::new(lsm_tree::compaction::Budgeted::new(
            local,
            EUSTRESS_MAX_SEGMENTS_PER_COMPACTION,
        ))
    }
}
