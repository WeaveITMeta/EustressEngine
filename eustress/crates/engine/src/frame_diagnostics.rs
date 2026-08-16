use bevy::prelude::*;
use std::time::{Duration, Instant};
use std::collections::HashMap;

/// Default hitch threshold in milliseconds.
///
/// This used to be 1000 ms (and was constructed at 2000 ms), which meant the
/// log only ever showed CATASTROPHIC freezes. The hitches that actually make
/// an editor feel broken — a 120 ms pause while dragging, a 300 ms hang on
/// select — were completely invisible, so "it lags and freezes" had no
/// corresponding evidence anywhere in the diagnostics. 100 ms is roughly the
/// point a pause stops reading as "slow" and starts reading as "stuck".
///
/// Override with `EUSTRESS_STUTTER_MS` (e.g. `250` to see only bigger hangs).
const DEFAULT_STUTTER_MS: u64 = 100;

/// How many frames between rolling percentile reports.
const REPORT_EVERY_FRAMES: u32 = 300;

fn stutter_threshold_ms() -> u64 {
    static V: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        std::env::var("EUSTRESS_STUTTER_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_STUTTER_MS)
    })
}

/// Resource tracking frame times and per-system execution times
#[derive(Resource)]
pub struct FrameTimeTracker {
    last_frame: Option<Instant>,
    stutter_threshold: Duration,
    system_times: HashMap<String, Duration>,
    current_system_start: Option<(String, Instant)>,
    /// Rolling window of recent frame times (microseconds) for percentiles.
    /// A mean alone hides hitches completely — a 2 s freeze every 300 frames
    /// adds only ~7 ms to the mean but is the whole user-visible problem.
    /// Fixed capacity, so this allocates once and never grows.
    recent_us: Vec<u32>,
    /// Frames since the last percentile report.
    since_report: u32,
    /// Hitches (over threshold) counted since the last report.
    hitches: u32,
}

impl Default for FrameTimeTracker {
    fn default() -> Self {
        Self::new(stutter_threshold_ms())
    }
}

impl FrameTimeTracker {
    pub fn new(stutter_threshold_ms: u64) -> Self {
        Self {
            last_frame: None,
            stutter_threshold: Duration::from_millis(stutter_threshold_ms),
            system_times: HashMap::new(),
            current_system_start: None,
            recent_us: Vec::with_capacity(REPORT_EVERY_FRAMES as usize),
            since_report: 0,
            hitches: 0,
        }
    }
    
    pub fn start_system(&mut self, name: String) {
        self.current_system_start = Some((name, Instant::now()));
    }
    
    pub fn end_system(&mut self) {
        if let Some((name, start)) = self.current_system_start.take() {
            let duration = start.elapsed();
            *self.system_times.entry(name).or_insert(Duration::ZERO) += duration;
        }
    }
}

/// System to track frame times and log stutters with per-system breakdown
pub fn track_frame_time(mut tracker: ResMut<FrameTimeTracker>) {
    let now = Instant::now();
    
    if let Some(last) = tracker.last_frame {
        let frame_time = now.duration_since(last);
        
        if frame_time > tracker.stutter_threshold {
            warn!(
                "⚠️ STUTTER DETECTED: Frame took {:.0}ms (threshold: {:.0}ms)",
                frame_time.as_secs_f64() * 1000.0,
                tracker.stutter_threshold.as_secs_f64() * 1000.0
            );
            
            // Log top 10 slowest systems this frame (only if instrumented)
            if !tracker.system_times.is_empty() {
                let mut sorted: Vec<_> = tracker.system_times.iter().collect();
                sorted.sort_by(|a, b| b.1.cmp(a.1));
                warn!("Top systems this frame:");
                for (name, duration) in sorted.iter().take(10) {
                    if duration.as_millis() > 10 {
                        warn!("  - {}: {:.1}ms", name, duration.as_secs_f64() * 1000.0);
                    }
                }
            }
        }
        
        // Clear system times for next frame
        tracker.system_times.clear();

        // ── Rolling percentile report ───────────────────────────────────────
        // Always on and effectively free (one push + a sort every 300 frames).
        // This is the number to judge "is the editor usable": p99 and max are
        // what a user feels, and a mean cannot show them.
        let us = frame_time.as_micros().min(u32::MAX as u128) as u32;
        tracker.recent_us.push(us);
        if frame_time > tracker.stutter_threshold {
            tracker.hitches += 1;
        }
        tracker.since_report += 1;
        if tracker.since_report >= REPORT_EVERY_FRAMES {
            let hitches = tracker.hitches;
            let thresh_ms = tracker.stutter_threshold.as_secs_f64() * 1000.0;
            let mut v = std::mem::take(&mut tracker.recent_us);
            v.sort_unstable();
            let pick = |q: f64| -> f64 {
                if v.is_empty() {
                    return 0.0;
                }
                let i = (((v.len() - 1) as f64) * q).round() as usize;
                v[i] as f64 / 1000.0
            };
            let n = v.len();
            info!(
                target: "eustress_engine::frame_diagnostics",
                "FRAME p50={:.1}ms p95={:.1}ms p99={:.1}ms max={:.1}ms | {} hitch(es) >{:.0}ms in {} frames",
                pick(0.50), pick(0.95), pick(0.99), pick(1.0), hitches, thresh_ms, n,
            );
            v.clear();
            tracker.recent_us = v; // reuse the allocation
            tracker.since_report = 0;
            tracker.hitches = 0;
        }
    }

    tracker.last_frame = Some(now);
}

/// Macro to wrap a system with timing
#[macro_export]
macro_rules! timed_system {
    ($tracker:expr, $name:expr, $system:expr) => {{
        $tracker.start_system($name.to_string());
        let result = $system;
        $tracker.end_system();
        result
    }};
}

/// DIAGNOSTIC (armed only when `EUSTRESS_PROFILE` is set): name the
/// entities whose `Instance` component is marked Changed each frame.
/// A steady-state world should have ZERO — the Mountain Ascension
/// profile showed a per-frame `Changed<Instance>` storm keeping four
/// downstream consumers (explorer sync, scene deltas, mention index,
/// snapshot extract) permanently hot. This names the writer.
fn trace_instance_change_storm(
    changed: Query<(Entity, &eustress_common::classes::Instance), Changed<eustress_common::classes::Instance>>,
    // Movers: what keeps transform propagation / scene deltas / extract
    // busy every frame. Named so per-frame animators (sun cycle, stars,
    // clouds, scripts) are identifiable.
    moved: Query<(Entity, Option<&Name>), Changed<Transform>>,
    time: Res<Time>,
    mut timer: Local<f32>,
) {
    static ARMED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ARMED.get_or_init(|| std::env::var("EUSTRESS_PROFILE").is_ok()) {
        return;
    }
    *timer += time.delta_secs();
    if *timer < 2.0 {
        return;
    }
    *timer = 0.0;
    let total = changed.iter().count();
    if total > 0 {
        let sample: Vec<String> = changed
            .iter()
            .take(5)
            .map(|(e, i)| format!("{:?}={}({:?})", e, i.name, i.class_name))
            .collect();
        warn!(
            "🔎 Changed<Instance> storm: {} changed this frame — sample: {}",
            total,
            sample.join(", ")
        );
    }
    let movers = moved.iter().count();
    if movers > 0 {
        let sample: Vec<String> = moved
            .iter()
            .take(6)
            .map(|(e, n)| {
                n.map(|n| format!("{:?}={}", e, n.as_str()))
                    .unwrap_or_else(|| format!("{:?}", e))
            })
            .collect();
        warn!(
            "🔎 Transform movers: {} changed this frame — sample: {}",
            movers,
            sample.join(", ")
        );
    }
}

pub struct FrameDiagnosticsPlugin;

impl Plugin for FrameDiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        // Threshold comes from `EUSTRESS_STUTTER_MS`, default 100 ms. The old
        // hardcoded 2000 ms meant an editor could hitch for a fifth of a second
        // on every drag and the logs would show a clean bill of health.
        app.insert_resource(FrameTimeTracker::default())
            .add_systems(Last, track_frame_time)
            // Perf diagnostic — dormant unless EUSTRESS_PROFILE is set.
            .add_systems(Update, trace_instance_change_storm);
    }
}
