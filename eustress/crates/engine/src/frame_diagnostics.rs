use bevy::prelude::*;
use bevy::ecs::schedule::ScheduleLabel;
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
        Self::new(100)
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
            
            // Log top 10 slowest systems this frame
            let mut sorted: Vec<_> = tracker.system_times.iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(a.1));
            
            warn!("Top systems this frame:");
            for (name, duration) in sorted.iter().take(10) {
                if duration.as_millis() > 10 {
                    warn!("  - {}: {:.1}ms", name, duration.as_secs_f64() * 1000.0);
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

pub struct FrameDiagnosticsPlugin;

impl Plugin for FrameDiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(FrameTimeTracker::new(100)) // 100ms threshold
            .add_systems(Last, track_frame_time);
    }
}
