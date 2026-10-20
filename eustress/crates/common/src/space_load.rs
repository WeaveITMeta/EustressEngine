//! # Whether a Space is still loading
//!
//! Shared by Studio's loader, which sets it, and the draw side, which
//! defers whole-scene work until the load settles.

use bevy::prelude::*;

/// Gate that suppresses `write_instance_changes_system` while the
/// loader is materialising entities from disk. Without it, downstream
/// systems that fire on first-load (mesh-handle resolve →
/// `update_base_part_size_from_mesh`, class-default backfill, material
/// registry resolve) mark `BasePart` as `Changed`, which the writer
/// then persists straight back to disk — at 50k entities that costs
/// ~53 s of background TOML writes for zero useful work.
///
/// Lifecycle:
/// - `load_space_files_system` (Startup) sets `active = true`.
/// - `apply_space_rescan` (Update) sets `active = true` on every rescan.
/// - `open_space` (in `space_ops`) sets `active = true` on Space switch.
/// - `tick_load_in_progress` (Update, runs after `load_deferred_services`)
///   increments `frames_since_quiescent` each frame the deferred queue is
///   empty and `priority_done`. Once the count reaches
///   `QUIESCENT_THRESHOLD`, `active` flips to false and live writes
///   resume.
#[derive(Resource, Debug, Default)]
pub struct LoadInProgress {
    pub active: bool,
    pub frames_since_quiescent: u32,
    /// When the deferred queue was last seen empty with priority done.
    pub quiescent_since: Option<std::time::Instant>,
}

impl LoadInProgress {
    /// The deferred queue must stay empty for BOTH of these before the load
    /// is declared settled: enough frames to absorb the async mesh-handle
    /// resolution + BasePart-size sync that runs after the last spawn, and
    /// enough wall time that the frame count means the same at any frame
    /// rate. The old 60-frame rule was sized for 16 ms frames; at the 300 ms
    /// frames of a large load it was an 18 s wait with write-back gated.
    pub const QUIESCENT_FRAMES: u32 = 3;
    pub const QUIESCENT_TIME: std::time::Duration = std::time::Duration::from_secs(1);

    /// Mark loading as active. Called by the load entry-points so the
    /// quiescent counter restarts whenever a fresh load begins.
    pub fn begin(&mut self) {
        self.active = true;
        self.frames_since_quiescent = 0;
        self.quiescent_since = None;
    }
}
