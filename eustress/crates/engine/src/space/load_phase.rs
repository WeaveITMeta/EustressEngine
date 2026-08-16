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
use std::sync::{Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

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

// ─────────────────────────────────────────────────────────────────────────────
// Unfinished-phase watchdog
// ─────────────────────────────────────────────────────────────────────────────
//
// [`mark`] above records milestones that HAPPENED. It cannot report the case
// that actually strands a user: a phase that BEGAN and never finished, with
// the main thread wedged inside it so no toast, no Slint dialog and no
// `Update` system can run. Opening a Space whose disk tree has ~1.3M files
// does exactly that — the reconcile walk holds the main thread, Windows paints
// "Not Responding", and the log's last line is whatever printed before the
// walk started.
//
// So this half is deliberately different from `mark`:
//
// * **Always armed.** Not gated on `EUSTRESS_PROFILE` — a hang the user is
//   staring at must report itself without them having known to set an env var
//   beforehand. Cost is one mutex push/remove per PHASE (a handful per load),
//   not per entity.
// * **Off-thread.** A dedicated watchdog thread owns the timing, because the
//   thread that would normally notice is the blocked one.
// * **Escalating.** Log first (works while frozen, tracing writes from this
//   thread), then a queued toast for when the main thread recovers, then a
//   native `rfd` dialog — the only surface that renders during a hard block,
//   since it does not need the Bevy schedule or the Slint event loop.

/// Default seconds a phase may run before the watchdog reports it stuck.
/// Generous: a genuinely large Space legitimately takes tens of seconds, and a
/// false popup is worse than a late one. Override with
/// `EUSTRESS_PHASE_WATCHDOG_SECS` (`0` disables the watchdog entirely).
const WATCHDOG_DEFAULT_SECS: u64 = 30;

/// A phase currently in flight.
struct PhaseEntry {
    name: &'static str,
    /// Context for the report — usually the Space path being opened.
    detail: String,
    started: Instant,
    /// Set once reported, so a stuck phase produces exactly one popup rather
    /// than one per watchdog tick.
    reported: bool,
}

fn in_flight() -> &'static Mutex<Vec<PhaseEntry>> {
    static IN_FLIGHT: OnceLock<Mutex<Vec<PhaseEntry>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(|| Mutex::new(Vec::new()))
}

/// Seconds before a phase is considered stuck; `None` when disabled.
fn watchdog_threshold() -> Option<Duration> {
    static CACHED: OnceLock<Option<Duration>> = OnceLock::new();
    *CACHED.get_or_init(|| {
        let secs = std::env::var("EUSTRESS_PHASE_WATCHDOG_SECS")
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(WATCHDOG_DEFAULT_SECS);
        (secs > 0).then(|| Duration::from_secs(secs))
    })
}

/// Open a phase. Pair with [`end`] on EVERY exit path — an early `return`
/// that skips `end` is itself reported as a stuck phase, which is the
/// intended behaviour (a phase that silently bailed is unfinished work).
pub fn begin(name: &'static str, detail: impl Into<String>) {
    if watchdog_threshold().is_none() {
        return;
    }
    start_watchdog();
    let mut g = in_flight().lock().unwrap_or_else(|e| e.into_inner());
    // Re-entering the same phase name replaces the older entry rather than
    // stacking, so a re-opened Space cannot leak a permanently-stuck ghost.
    g.retain(|e| e.name != name);
    g.push(PhaseEntry {
        name,
        detail: detail.into(),
        started: Instant::now(),
        reported: false,
    });
}

/// Close a phase opened by [`begin`]. Logs the duration when it was slow
/// enough to be worth seeing, and always when the watchdog already reported
/// it — so the log shows the resolution, not just the alarm.
pub fn end(name: &'static str) {
    if watchdog_threshold().is_none() {
        return;
    }
    let finished = {
        let mut g = in_flight().lock().unwrap_or_else(|e| e.into_inner());
        let idx = g.iter().position(|e| e.name == name);
        idx.map(|i| g.remove(i))
    };
    let Some(entry) = finished else { return };
    let elapsed = entry.started.elapsed();
    if entry.reported {
        bevy::log::warn!(
            target: "eustress_engine::load_phase",
            "PHASE '{}' finally completed after {:.1}s (was reported stuck)",
            entry.name,
            elapsed.as_secs_f32()
        );
        crate::notifications::notify_from_background(
            crate::notifications::NotificationLevel::Info,
            format!(
                "'{}' finished after {:.0}s",
                entry.name,
                elapsed.as_secs_f32()
            ),
        );
    } else if elapsed >= Duration::from_secs(5) {
        bevy::log::info!(
            target: "eustress_engine::load_phase",
            "PHASE '{}' completed in {:.1}s",
            entry.name,
            elapsed.as_secs_f32()
        );
    }
}

/// RAII handle returned by [`scope`]. Closes its phase on drop, so every
/// exit path — early `return`, `?`, or a panic unwinding through — is covered
/// without the call site enumerating them.
pub struct PhaseGuard(&'static str);

impl Drop for PhaseGuard {
    fn drop(&mut self) {
        end(self.0);
    }
}

/// Open a phase bound to the enclosing scope. Prefer this over bare
/// [`begin`]/[`end`] for anything with more than one exit path — a missed
/// `end` is indistinguishable from a hang and would fire a false popup.
///
/// ```ignore
/// let _phase = load_phase::scope("space-open", path.display().to_string());
/// ```
#[must_use = "the phase closes when this guard drops; binding to `_` closes it immediately"]
pub fn scope(name: &'static str, detail: impl Into<String>) -> PhaseGuard {
    begin(name, detail);
    PhaseGuard(name)
}

/// Spawn the watchdog thread. Idempotent — the first [`begin`] starts it and
/// every later call is a no-op, so nothing needs to own its lifecycle.
fn start_watchdog() {
    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        let Some(threshold) = watchdog_threshold() else {
            return;
        };
        let spawned = std::thread::Builder::new()
            .name("eustress-phase-watchdog".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(1000));
                // Collect under the lock, report outside it — reporting can
                // block (the dialog), and holding the lock across that would
                // stall every `begin`/`end` on the main thread.
                //
                // Phases overlap: `space-open` encloses `db-reconcile`, and
                // `gaussian-splat-scan` runs from a different system entirely,
                // so several can be stuck at once. Picking the innermost PER
                // TICK is not enough — phases that cross the threshold on
                // different ticks (30s, 30s, 31s) each fired their own dialog,
                // which is what three stacked popups looked like in practice.
                //
                // So the dialog is latched GLOBALLY instead: at most one on
                // screen, and it names every phase currently stuck rather than
                // guessing which one matters. Logs and toasts are per-phase and
                // stay uncapped — only the modal is deduplicated.
                let newly_stuck: Vec<(&'static str, String, f32)> = {
                    let mut g = in_flight().lock().unwrap_or_else(|e| e.into_inner());
                    g.iter_mut()
                        .filter(|e| !e.reported && e.started.elapsed() >= threshold)
                        .map(|e| {
                            e.reported = true;
                            (e.name, e.detail.clone(), e.started.elapsed().as_secs_f32())
                        })
                        .collect()
                };
                if newly_stuck.is_empty() {
                    continue;
                }
                // Log + toast every one — cheap, non-modal, and the log is the
                // record that survives the session.
                for (name, detail, secs) in &newly_stuck {
                    log_and_toast_stuck(name, detail, *secs);
                }
                // One modal, describing everything stuck right now (including
                // phases reported on an earlier tick and still running).
                if DIALOG_OPEN
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    let all_stuck: Vec<(&'static str, String, f32)> = {
                        let g = in_flight().lock().unwrap_or_else(|e| e.into_inner());
                        g.iter()
                            .filter(|e| e.started.elapsed() >= threshold)
                            .map(|e| {
                                (e.name, e.detail.clone(), e.started.elapsed().as_secs_f32())
                            })
                            .collect()
                    };
                    show_stuck_dialog(all_stuck);
                }
            });
        if spawned.is_err() {
            bevy::log::warn!(
                target: "eustress_engine::load_phase",
                "phase watchdog thread failed to spawn — stuck phases will not be reported"
            );
        }
    });
}

/// True while a watchdog dialog is on screen. Latches the modal so overlapping
/// phases crossing the threshold on different ticks cannot stack dialogs.
static DIALOG_OPEN: AtomicBool = AtomicBool::new(false);

/// Log + toast one stuck phase. Non-modal, so this is uncapped — every stuck
/// phase gets a record even though they share a single dialog.
fn log_and_toast_stuck(name: &str, detail: &str, secs: f32) {
    // The log is the only surface guaranteed to work while the main thread is
    // blocked, since this runs on the watchdog thread.
    bevy::log::error!(
        target: "eustress_engine::load_phase",
        "⏳ PHASE '{name}' UNFINISHED after {secs:.0}s — {detail}. \
         The main thread is still inside this phase; the window may show \
         'Not Responding' until it completes."
    );
    // Queued, so it appears whenever the main thread next pumps. Useless
    // during a hard block, correct for a merely slow phase.
    crate::notifications::notify_from_background(
        crate::notifications::NotificationLevel::Error,
        format!("'{name}' unfinished after {secs:.0}s — still working"),
    );
}

/// Show the single watchdog dialog, describing every phase currently stuck.
///
/// Native `rfd` rather than a Slint dialog or a Bevy-side toast: it needs
/// neither the Bevy schedule nor the Slint event loop, so it is the one
/// surface that renders while the main thread is blocked — which is exactly
/// the case worth reporting. Runs on its own thread because `show()` blocks
/// until dismissed and the watchdog has to keep ticking. Clears [`DIALOG_OPEN`]
/// on dismissal so a later hang can raise a fresh one.
fn show_stuck_dialog(stuck: Vec<(&'static str, String, f32)>) {
    let mut body = if stuck.len() == 1 {
        String::from("A load phase has been running without finishing:\n\n")
    } else {
        format!(
            "{} load phases have been running without finishing:\n\n",
            stuck.len()
        )
    };
    for (name, detail, secs) in &stuck {
        body.push_str(&format!("  • {name} — {secs:.0}s\n      {detail}\n"));
    }
    body.push_str(
        "\nThe engine is still working — this is not a crash. The window may be \
         unresponsive until these complete.\n\n\
         Raise or disable the threshold with EUSTRESS_PHASE_WATCHDOG_SECS \
         (seconds; 0 disables).",
    );

    let spawned = std::thread::Builder::new()
        .name("eustress-phase-watchdog-dialog".into())
        .spawn(move || {
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Warning)
                .set_title("Eustress — load phase still running")
                .set_description(&body)
                .set_buttons(rfd::MessageButtons::Ok)
                .show();
            DIALOG_OPEN.store(false, Ordering::SeqCst);
        });
    if spawned.is_err() {
        // Never leave the latch stuck closed — that would silence every later
        // report for the rest of the session.
        DIALOG_OPEN.store(false, Ordering::SeqCst);
    }
}
