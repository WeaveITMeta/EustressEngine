use bevy::prelude::*;
use std::time::{Duration, Instant};
use std::collections::HashMap;

/// Resource tracking frame times and per-system execution times
#[derive(Resource)]
pub struct FrameTimeTracker {
    last_frame: Option<Instant>,
    stutter_threshold: Duration,
    system_times: HashMap<String, Duration>,
    current_system_start: Option<(String, Instant)>,
}

impl Default for FrameTimeTracker {
    fn default() -> Self {
        Self::new(1000) // Only log frames over 1 second
    }
}

impl FrameTimeTracker {
    pub fn new(stutter_threshold_ms: u64) -> Self {
        Self {
            last_frame: None,
            stutter_threshold: Duration::from_millis(stutter_threshold_ms),
            system_times: HashMap::new(),
            current_system_start: None,
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
        app.insert_resource(FrameTimeTracker::new(2000)) // 2 second threshold — only log severe stutters
            .add_systems(Last, track_frame_time)
            // Perf diagnostic — dormant unless EUSTRESS_PROFILE is set.
            .add_systems(Update, trace_instance_change_storm);
    }
}
