use bevy::prelude::*;
use std::time::{Duration, Instant};

/// Resource tracking frame times to detect stutters
#[derive(Resource, Default)]
pub struct FrameTimeTracker {
    last_frame: Option<Instant>,
    stutter_threshold: Duration,
}

impl FrameTimeTracker {
    pub fn new(stutter_threshold_ms: u64) -> Self {
        Self {
            last_frame: None,
            stutter_threshold: Duration::from_millis(stutter_threshold_ms),
        }
    }
}

/// System to track frame times and log stutters
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
        }
    }
    
    tracker.last_frame = Some(now);
}

pub struct FrameDiagnosticsPlugin;

impl Plugin for FrameDiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(FrameTimeTracker::new(100)) // 100ms threshold
            .add_systems(Last, track_frame_time);
    }
}
