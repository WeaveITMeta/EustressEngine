//! Opt-in per-system frame micro-profiler.
//!
//! Goal: find what is eating the frame budget when a very large scene
//! (hundreds of thousands of entities) is loaded, by attributing wall-clock
//! time to individual Bevy **systems** across every schedule.
//!
//! ## How it works
//!
//! Bevy wraps each system's execution in a `tracing` span named `"system"`
//! (and `"system_commands"` for the deferred-`Commands` apply phase), each
//! carrying a `name` field holding the system's full path. Those spans only
//! exist when `bevy_ecs` is built with its `trace` feature — which is why the
//! engine's `profiling` feature turns on `bevy/trace` (see `Cargo.toml`).
//!
//! We install a [`tracing_subscriber::Layer`] into the SAME global subscriber
//! that Bevy's `LogPlugin` builds (via its `custom_layer` hook), so we share
//! one dispatcher with the whole process — including the render sub-app, whose
//! extract / prepare / queue systems run on the same global `tracing`
//! dispatcher and therefore flow through this layer too.
//!
//! For each `"system"` span the layer records `enter` → `exit` wall time and
//! accumulates, per system name, a running `(total Duration, call count)` over
//! a rolling window of N frames. Once the window closes (or a one-shot capture
//! fires after the scene settles) it writes two artifacts to the current
//! working directory:
//!
//! * `eustress_profile.txt` — a ranked table (rank, system, total ms over the
//!   window, avg ms/frame, % of mean frame time, call count) for an AI/human
//!   to read. The top 20 are also logged at `warn!`/`info!` so they land in
//!   captured stdout.
//! * `eustress_profile.svg` — an `inferno` flamegraph built from one folded
//!   stack line per system (`system_name total_micros`), for a human to open.
//!
//! ## Cost when off
//!
//! * Feature `profiling` **off** (default): this module is an empty plugin.
//!   `bevy/trace` is not enabled, so Bevy never even constructs the system
//!   spans — there is nothing to observe and zero overhead.
//! * Feature **on** but env `EUSTRESS_PROFILE` **unset**: the layer is
//!   installed but every callback early-returns on an atomic load, and the
//!   per-layer filter rejects all callsites, so the practical cost is a single
//!   relaxed atomic read on the system-span callsite path.
//! * Feature **on** + `EUSTRESS_PROFILE=1`: full capture + periodic dump.
//!
//! Env knobs:
//! * `EUSTRESS_PROFILE` — set to any non-empty value to arm capture.
//! * `EUSTRESS_PROFILE_FRAMES` — window length in frames (default 120).

// `App`/`Plugin` are used at this scope only by the feature-off stubs below;
// the feature-on path imports the prelude inside `mod enabled`. Scoping the
// import to the off path keeps both configurations warning-free.
#[cfg(not(feature = "profiling"))]
use bevy::prelude::*;

/// Bevy plugin that arms the per-system profiler.
///
/// When the `profiling` feature is disabled this is an inert marker whose
/// `build` does nothing — added unconditionally in `main.rs` so the call site
/// never needs its own `#[cfg]`.
pub struct ProfilerPlugin;

// ───────────────────────────── feature OFF ──────────────────────────────
// Empty implementation. No layer, no resources, no systems. The
// `custom_layer` hook in `main.rs` is also `#[cfg]`-gated to `|_| None` so the
// LogPlugin wiring compiles identically with the feature off.
#[cfg(not(feature = "profiling"))]
impl Plugin for ProfilerPlugin {
    fn build(&self, _app: &mut App) {}
}

/// The `LogPlugin::custom_layer` hook value.
///
/// With the feature **off** this resolves to `|_| None` so the LogPlugin
/// builds exactly as before. With the feature **on** it returns our boxed,
/// self-filtered profiling layer. `main.rs` plugs this into
/// `LogPlugin { custom_layer: profiler::custom_layer, .. }` unconditionally.
#[cfg(not(feature = "profiling"))]
pub fn custom_layer(_app: &mut App) -> Option<bevy::log::BoxedLayer> {
    None
}

// ────────────────────────────── feature ON ──────────────────────────────
#[cfg(feature = "profiling")]
pub use enabled::custom_layer;

#[cfg(feature = "profiling")]
mod enabled {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use bevy::log::BoxedLayer;
    use bevy::prelude::*;
    // Use Bevy's own re-exported `tracing_subscriber` so the `Registry` /
    // `Layer` types are byte-identical to those `BoxedLayer` expects. Pulling
    // a second `tracing_subscriber` as a direct dep risks a version skew that
    // would make the boxed layer fail to unify with `Layer<Registry>`.
    use bevy::log::tracing_subscriber::{
        filter::filter_fn,
        layer::{Context, Layer},
        registry::LookupSpan,
    };
    // `Field`/`Visit` come from the `tracing` facade (re-exporting
    // `tracing_core::field::*`). `tracing_subscriber::field` re-exports `Visit`
    // but NOT `Field`, so pulling both from `tracing::field` keeps a single,
    // version-unified source that matches the span field types Bevy emits.
    use tracing::field::{Field, Visit};
    use tracing::span;

    use super::ProfilerPlugin;

    /// Output file names, written to the process CWD.
    const REPORT_TXT: &str = "eustress_profile.txt";
    const REPORT_SVG: &str = "eustress_profile.svg";
    /// Default rolling window length in frames.
    const DEFAULT_WINDOW_FRAMES: u64 = 120;
    /// How many rows to put in the text/SVG report.
    const REPORT_TOP_N: usize = 60;
    /// How many rows to echo into the engine log.
    const LOG_TOP_N: usize = 20;

    /// Process-global profiler state, reachable from the non-capturing
    /// `custom_layer` fn pointer (which cannot close over anything).
    static PROFILER: OnceLock<Arc<ProfilerState>> = OnceLock::new();

    /// Get-or-init the global state, reading env knobs exactly once.
    fn state() -> &'static Arc<ProfilerState> {
        PROFILER.get_or_init(|| {
            let armed = std::env::var_os("EUSTRESS_PROFILE")
                .map(|v| !v.is_empty())
                .unwrap_or(false);
            let window = std::env::var("EUSTRESS_PROFILE_FRAMES")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|n| *n > 0)
                .unwrap_or(DEFAULT_WINDOW_FRAMES);
            Arc::new(ProfilerState {
                armed: AtomicBool::new(armed),
                window_frames: window,
                frames_in_window: AtomicU64::new(0),
                acc: Mutex::new(HashMap::new()),
            })
        })
    }

    /// Per-system accumulation: total wall time and number of executions
    /// observed within the current window.
    #[derive(Default, Clone, Copy)]
    struct Tally {
        total: Duration,
        calls: u64,
    }

    struct ProfilerState {
        /// Whether `EUSTRESS_PROFILE` armed capture. When false every hot-path
        /// callback returns immediately.
        armed: AtomicBool,
        /// Rolling window length in frames.
        window_frames: u64,
        /// Frames elapsed in the current window (bumped by the Bevy `Last`
        /// system; the layer never touches this).
        frames_in_window: AtomicU64,
        /// `system name -> (total, calls)` for the current window. Written
        /// from many system threads on every `on_exit`, drained on dump.
        acc: Mutex<HashMap<String, Tally>>,
    }

    impl ProfilerState {
        #[inline]
        fn is_armed(&self) -> bool {
            self.armed.load(Ordering::Relaxed)
        }
    }

    // Span-extension payloads. We stash the system name (captured from the
    // span's `name` field at creation) and the most-recent enter `Instant`
    // directly on the span via the registry's typed extension map, so the
    // hot path is just a typed get/insert — no name re-formatting per frame.
    struct SystemName(String);
    struct EnterAt(Instant);

    /// Visitor that lifts the `name` field out of a system span's attributes.
    /// Bevy records it as `name = <string>`; depending on the call site that
    /// arrives as either a string or a `Debug` value, so handle both.
    #[derive(Default)]
    struct NameVisitor {
        name: Option<String>,
    }

    impl Visit for NameVisitor {
        fn record_str(&mut self, field: &Field, value: &str) {
            if field.name() == "name" {
                self.name = Some(value.to_owned());
            }
        }

        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            if field.name() == "name" && self.name.is_none() {
                // Trim the surrounding quotes a `Debug` string would add.
                let s = format!("{value:?}");
                let s = s.trim_matches('"').to_owned();
                self.name = Some(s);
            }
        }
    }

    /// The profiling [`Layer`]. Holds an `Arc` to the global state so its
    /// callbacks can accumulate without going through the `OnceLock` each
    /// time. Construction is the only place that reaches the static.
    struct SystemTimingLayer {
        state: Arc<ProfilerState>,
    }

    impl<S> Layer<S> for SystemTimingLayer
    where
        S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    {
        fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
            if !self.state.is_armed() {
                return;
            }
            let Some(span) = ctx.span(id) else { return };
            let mut visitor = NameVisitor::default();
            attrs.record(&mut visitor);
            // Fall back to the span's static metadata name if the field was
            // absent for some reason — never panic, never skip silently.
            let name = visitor
                .name
                .unwrap_or_else(|| span.metadata().name().to_owned());
            span.extensions_mut().insert(SystemName(name));
        }

        fn on_enter(&self, id: &span::Id, ctx: Context<'_, S>) {
            if !self.state.is_armed() {
                return;
            }
            if let Some(span) = ctx.span(id) {
                // Overwrite any previous enter stamp: a system span is only
                // entered on one thread at a time, so the last enter wins and
                // pairs with the next exit.
                span.extensions_mut().replace(EnterAt(Instant::now()));
            }
        }

        fn on_exit(&self, id: &span::Id, ctx: Context<'_, S>) {
            if !self.state.is_armed() {
                return;
            }
            let now = Instant::now();
            let Some(span) = ctx.span(id) else { return };
            let ext = span.extensions();
            let Some(EnterAt(started)) = ext.get::<EnterAt>() else {
                return;
            };
            let elapsed = now.saturating_duration_since(*started);
            // Resolve the system name captured at span creation.
            let name: String = match ext.get::<SystemName>() {
                Some(SystemName(n)) => n.clone(),
                None => span.metadata().name().to_owned(),
            };
            drop(ext);

            if let Ok(mut map) = self.state.acc.lock() {
                let tally = map.entry(name).or_default();
                tally.total += elapsed;
                tally.calls += 1;
            }
        }
    }

    /// Build the boxed, self-filtered profiling layer for `LogPlugin`.
    ///
    /// The per-layer `filter_fn` restricts this layer to the two Bevy system
    /// span names so it ignores every other span/event regardless of the
    /// global `EnvFilter`, and so disabled-callsite caching keeps the cost on
    /// unrelated spans at zero. When capture is not armed the filter rejects
    /// even the system spans, collapsing the layer to a no-op.
    pub fn custom_layer(_app: &mut App) -> Option<BoxedLayer> {
        let st = state().clone();
        let armed = st.is_armed();
        if armed {
            info!(
                "profiler: EUSTRESS_PROFILE armed — capturing per-system timings over {}-frame windows; \
                 writing {REPORT_TXT} + {REPORT_SVG} to the working directory",
                st.window_frames
            );
        } else {
            // Installed-but-idle: confirm the build has the capability so a
            // user knows the knob exists. Cheap, one line, at startup only.
            info!("profiler: profiling feature built; set EUSTRESS_PROFILE=1 to capture per-system timings");
        }

        // Filter state for the closure: a cheap clone of the arm flag handle.
        let filter_state = st.clone();
        let layer = SystemTimingLayer { state: st }.with_filter(filter_fn(move |meta| {
            // Only care about spans (not events), only the two system spans,
            // and only while armed.
            if !filter_state.is_armed() {
                return false;
            }
            if !meta.is_span() {
                return false;
            }
            matches!(meta.name(), "system" | "system_commands")
        }));

        Some(Box::new(layer))
    }

    // ─────────────────────────── Bevy plugin ────────────────────────────
    impl Plugin for ProfilerPlugin {
        fn build(&self, app: &mut App) {
            // Touch the global state so the env vars are read and the
            // arm/window decision is logged in a single, predictable place
            // even if `custom_layer` ran first (idempotent via OnceLock).
            let _ = state();
            // The window-tick + dump runs in `Last`, after every other
            // schedule for the frame has executed, so a window boundary
            // observes a complete frame of system timings.
            app.add_systems(Last, tick_and_maybe_dump);
        }
    }

    /// Bevy system: advance the window counter and, on a window boundary,
    /// drain the accumulator and write the report + flamegraph.
    fn tick_and_maybe_dump() {
        let st = state();
        if !st.is_armed() {
            return;
        }
        let n = st.frames_in_window.fetch_add(1, Ordering::Relaxed) + 1;
        if n < st.window_frames {
            return;
        }
        // Window closed: reset the counter and take a snapshot of the tallies.
        st.frames_in_window.store(0, Ordering::Relaxed);
        let snapshot: Vec<(String, Tally)> = {
            let mut map = match st.acc.lock() {
                Ok(m) => m,
                Err(_) => return,
            };
            let out = map.iter().map(|(k, v)| (k.clone(), *v)).collect();
            map.clear();
            out
        };
        if snapshot.is_empty() {
            return;
        }
        dump(snapshot, st.window_frames);
    }

    /// Rank, write `eustress_profile.txt`, render `eustress_profile.svg`, and
    /// echo the top entries into the engine log.
    fn dump(mut rows: Vec<(String, Tally)>, window_frames: u64) {
        // Sort by total time descending — the slowest system first.
        rows.sort_by(|a, b| b.1.total.cmp(&a.1.total));

        // Mean frame time over the window = sum of all per-system time divided
        // by frame count. This is the denominator for the "% of frame" column.
        // (With Bevy's multi-threaded executor, summed system time can exceed
        // wall time because systems overlap; the percentage is therefore a
        // share of total CPU-system-time, which is still the right signal for
        // "which system dominates", and we say so in the header.)
        let total_all: Duration = rows.iter().map(|(_, t)| t.total).sum();
        let mean_frame_ms = (total_all.as_secs_f64() * 1000.0) / window_frames as f64;
        let denom_ms = if mean_frame_ms > 0.0 { mean_frame_ms } else { 1.0 };

        // ---- text report ----
        let mut text = String::new();
        text.push_str(&format!(
            "Eustress per-system profile — window = {window_frames} frames, {} systems observed\n",
            rows.len()
        ));
        text.push_str(&format!(
            "Mean summed system-time per frame: {mean_frame_ms:.2} ms (sum across overlapping threads)\n",
        ));
        text.push_str("rank  total_ms   avg_ms/frame   %frame   calls   system\n");
        text.push_str("----  --------   ------------   ------   -----   ------\n");
        for (i, (name, tally)) in rows.iter().take(REPORT_TOP_N).enumerate() {
            let total_ms = tally.total.as_secs_f64() * 1000.0;
            let avg_ms = total_ms / window_frames as f64;
            let pct = (avg_ms / denom_ms) * 100.0;
            text.push_str(&format!(
                "{:>4}  {:>8.2}   {:>12.3}   {:>5.1}%   {:>5}   {}\n",
                i + 1,
                total_ms,
                avg_ms,
                pct,
                tally.calls,
                name,
            ));
        }

        match std::fs::write(REPORT_TXT, &text) {
            Ok(()) => info!("profiler: wrote {REPORT_TXT} ({} systems)", rows.len()),
            Err(e) => warn!("profiler: failed writing {REPORT_TXT}: {e}"),
        }

        // ---- log echo (top N) ----
        warn!(
            "profiler: top {} systems by total time over {} frames (mean summed system-time {:.2} ms/frame):",
            LOG_TOP_N.min(rows.len()),
            window_frames,
            mean_frame_ms,
        );
        for (i, (name, tally)) in rows.iter().take(LOG_TOP_N).enumerate() {
            let total_ms = tally.total.as_secs_f64() * 1000.0;
            let avg_ms = total_ms / window_frames as f64;
            info!(
                "  #{:>2}  {:>8.2} ms total  {:>8.3} ms/frame  x{:<5}  {}",
                i + 1,
                total_ms,
                avg_ms,
                tally.calls,
                name,
            );
        }

        // ---- inferno flamegraph ----
        render_flamegraph(&rows);
    }

    /// Build folded-stack lines (`system_name total_micros`) and render them
    /// to `eustress_profile.svg` with `inferno`. Each system is a single,
    /// flat stack frame; the flamegraph degenerates to a sorted bar chart of
    /// per-system cost, which is exactly the "what's eating the frame" view.
    fn render_flamegraph(rows: &[(String, Tally)]) {
        // inferno splits a folded line on the LAST whitespace into
        // `stack` + `count`, and splits the stack on ';'. System names contain
        // neither problematic spaces in a way that breaks the trailing-count
        // split (the count is appended after a single space), but they DO
        // contain characters fine for a leaf frame. Sanitize ';' just in case
        // a closure name embeds one.
        let mut folded = String::new();
        for (name, tally) in rows {
            let micros = tally.total.as_micros();
            if micros == 0 {
                continue;
            }
            let leaf = name.replace(';', ":");
            folded.push_str(&format!("{leaf} {micros}\n"));
        }
        if folded.is_empty() {
            return;
        }

        let file = match std::fs::File::create(REPORT_SVG) {
            Ok(f) => f,
            Err(e) => {
                warn!("profiler: failed creating {REPORT_SVG}: {e}");
                return;
            }
        };
        let writer = std::io::BufWriter::new(file);

        let mut opts = inferno::flamegraph::Options::default();
        opts.title = "Eustress per-system frame profile".to_string();
        opts.subtitle = Some("total microseconds per system over the sample window".to_string());
        opts.count_name = "µs".to_string();
        // Keep the original (time-sorted) order so the SVG reads top-down like
        // the text report rather than alphabetically.
        opts.no_sort = true;

        let lines = folded.lines();
        match inferno::flamegraph::from_lines(&mut opts, lines, writer) {
            Ok(()) => info!("profiler: wrote {REPORT_SVG}"),
            Err(e) => warn!("profiler: inferno flamegraph render failed: {e}"),
        }
    }
}
