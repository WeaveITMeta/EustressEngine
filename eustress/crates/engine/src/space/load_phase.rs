//! Lightweight, always-compiled load-phase instrumentation.
//!
//! Measures wall-clock elapsed since a Space load began, at a handful of
//! milestones along the open→interactive pipeline, so the ~50s a large
//! import (e.g. the 161K-file Vehicle Simulator place) spends getting to
//! interactive can be attributed to a phase. The analysis flagged the
//! *missing per-phase breakdown* as the #1 gap; this is that breakdown.
//!
//! ## Design (mirrors `crate::profiler` phase profiler)
//!
//! * **Always compiled** — no feature flag, no Bevy rebuild. Dormant until
//!   `EUSTRESS_PROFILE` is set (the same env knob the phase profiler reads),
//!   guarded by a single `OnceLock<bool>` read per call.
//! * **Process-global** — a `OnceLock<Instant>` "space-open start" stamped
//!   the moment a Space load begins, plus an `AtomicU64` holding the
//!   milliseconds-since-start of the previous milestone (for the Δ column).
//!   Both are static so any call site (`file_loader`, `residency`,
//!   `world_db_plugin`, all of which sit in different feature-gated modules)
//!   can mark a milestone with no resource plumbing or system ordering.
//! * **Cheap when off** — `mark()` does one relaxed `OnceLock` read and
//!   returns; it never reads the clock or formats a string unless armed.
//!
//! Each milestone logs one line:
//! ```text
//! LOAD-PHASE <name>: <ms>ms (Δ<delta>ms)
//! ```
//! where `<ms>` is elapsed since space-open begin and `<delta>` is elapsed
//! since the previous milestone.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

/// Read `EUSTRESS_PROFILE` exactly once; instrumentation is armed iff it is
/// non-empty. Identical semantics to `crate::profiler::phase_armed` so a
/// single env var arms both the per-phase frame profiler and these
/// load-phase milestones.
fn armed() -> bool {
    static ARMED: OnceLock<bool> = OnceLock::new();
    *ARMED.get_or_init(|| {
        std::env::var_os("EUSTRESS_PROFILE")
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    })
}

/// The instant the current Space load began. Stamped by [`stamp_open_start`]
/// at the head of `open_world_db_on_space_change` (DB-backed) and re-stamped
/// idempotently on a genuine new load.
static OPEN_START: OnceLock<std::sync::Mutex<Option<Instant>>> = OnceLock::new();

/// Milliseconds-since-`OPEN_START` recorded at the previous milestone — the
/// base for the Δ column. Reset to 0 by [`stamp_open_start`].
static LAST_MS: AtomicU64 = AtomicU64::new(0);

/// Set once `first_render` has fired so the one-shot frame milestone logs
/// exactly once per load, not every frame.
static FIRST_RENDER_DONE: AtomicBool = AtomicBool::new(false);

fn start_slot() -> &'static std::sync::Mutex<Option<Instant>> {
    OPEN_START.get_or_init(|| std::sync::Mutex::new(None))
}

/// Stamp the space-open start. Called at the very beginning of a Space load
/// (DB open / space change). Resets the Δ base and the one-shot first-render
/// latch so each Space switch re-measures from zero. A no-op cost-wise when
/// not armed (still cheap: one mutex + two stores, once per load).
pub fn stamp_open_start() {
    if !armed() {
        return;
    }
    *start_slot().lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    LAST_MS.store(0, Ordering::Relaxed);
    FIRST_RENDER_DONE.store(false, Ordering::Relaxed);
    // Milestone 1 itself: prove the stamp fired and anchor the timeline.
    bevy::log::info!(target: "eustress_engine::load_phase", "LOAD-PHASE space-open-begin: 0ms (Δ0ms)");
}

/// Log a milestone: elapsed-since-open and Δ-since-previous-milestone.
/// Silent (one `OnceLock` read) when `EUSTRESS_PROFILE` is unset, or if no
/// start was stamped this load.
pub fn mark(name: &str) {
    if !armed() {
        return;
    }
    let start = { *start_slot().lock().unwrap_or_else(|e| e.into_inner()) };
    let Some(start) = start else {
        return; // no load in flight (or stamp missed) — don't emit a bogus 0
    };
    let now_ms = start.elapsed().as_millis() as u64;
    let prev = LAST_MS.swap(now_ms, Ordering::Relaxed);
    let delta = now_ms.saturating_sub(prev);
    bevy::log::info!(
        target: "eustress_engine::load_phase",
        "LOAD-PHASE {name}: {now_ms}ms (Δ{delta}ms)"
    );
}

/// One-shot first-rendered-frame milestone. Call every frame from a cheap
/// `Update` system once the camera is up; it self-latches so the line is
/// emitted exactly once per load (on the first call after `min_frame`).
/// `frame` is a since-stamp frame count so we can wait for N>=2 (the first
/// frame is the pipeline-warmup outlier the analysis flagged).
pub fn mark_first_render(frame: u64, min_frame: u64) {
    if !armed() {
        return;
    }
    if frame < min_frame {
        return;
    }
    // Latch: only the first caller past the gate emits.
    if FIRST_RENDER_DONE
        .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        mark("first-rendered-frame");
    }
}

/// Cheap always-added `Update` system that fires the one-shot
/// first-rendered-frame milestone (milestone 7). Uses a `Local` frame
/// counter rather than Bevy's `FrameCount` so it has no extra resource
/// dependency, and resets when a fresh load is stamped (the milestone
/// latch is cleared by [`stamp_open_start`]). Returns immediately when not
/// armed (one `OnceLock` read).
pub fn sys_mark_first_render(mut frame: bevy::prelude::Local<u64>) {
    if !armed() {
        return;
    }
    *frame = frame.saturating_add(1);
    // Wait for N>=2: frame 1 is the pipeline-warmup outlier. If a fresh
    // load re-armed the latch (stamp_open_start), the next qualifying frame
    // re-fires. We don't reset `frame` per load — the >=2 gate only matters
    // for the very first measurement; later loads fire on the first frame
    // after stamp because the latch was cleared and `*frame` is already >=2.
    mark_first_render(*frame, 2);
}
