//! # The coupled frame: one ordered graph for streaming, LOD, AI and physics
//!
//! RAGE schedules streaming, physics and AI as jobs in one dependency graph
//! per frame, so the expensive pieces overlap and the critical path is
//! known. Eustress's first slice of that (phase 3 of
//! `docs/architecture/RAGE_STREAMING_SIM_LOD_JOB_GRAPH.md`) is explicit and
//! measured order, without moving any existing system:
//!
//! ```text
//! RunFixedMainLoop  Avian PhysicsSchedule (0..n fixed steps)      "physics"
//! Update            WorldSimSet::Predict -> Stream -> Tier        "world_sim"
//! Update            PlayScriptSet::Pull -> Scripts -> Apply -> End "play"
//!                   (NPC AI runs in Apply; its velocities feed the
//!                    next frame's physics step)
//! ```
//!
//! [`FrameGraphTimeline`] times each stage with marker systems at its seams
//! and keeps an average, a maximum and the frame's longest stage, which the
//! inspector reports as the critical path. Markers are wall-clock at seams:
//! in Bevy's multi-threaded executor a stage's span includes whatever ran
//! beside it, so the numbers bound the stage from above.

use bevy::app::RunFixedMainLoopSystems;
use bevy::platform::time::Instant;
use bevy::prelude::*;
use serde::Serialize;

use crate::play_session::PlayScriptSet;

/// The world-simulation stages of the `Update` schedule, in order.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorldSimSet {
    /// Sample foci and plan what each streamer needs next.
    Predict,
    /// Streamers promote, load and evict.
    Stream,
    /// Simulation LOD decides and applies tiers.
    Tier,
}

/// Order the world-simulation sets. Safe to call from every plugin that
/// uses them.
pub fn configure_world_sim_sets(app: &mut App) {
    app.configure_sets(Update, (WorldSimSet::Predict, WorldSimSet::Stream, WorldSimSet::Tier).chain());
}

/// The stages the timeline measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Physics = 0,
    WorldSim = 1,
    Play = 2,
}

const STAGE_NAMES: [&str; 3] = ["physics", "world_sim", "play"];

/// One stage's timing.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq)]
pub struct StageTiming {
    pub last_ms: f32,
    pub avg_ms: f32,
    pub max_ms: f32,
    pub samples: u64,
}

impl StageTiming {
    /// Fold one measurement in: EMA with weight 0.1, running max.
    pub fn record(&mut self, ms: f32) {
        if !ms.is_finite() || ms < 0.0 {
            return;
        }
        self.last_ms = ms;
        self.avg_ms = if self.samples == 0 { ms } else { self.avg_ms * 0.9 + ms * 0.1 };
        self.max_ms = self.max_ms.max(ms);
        self.samples += 1;
    }
}

/// Per-stage timings and the frame they sit in.
#[derive(Resource, Debug, Clone, Default)]
pub struct FrameGraphTimeline {
    stages: [StageTiming; 3],
    open: [Option<Instant>; 3],
    frame_start: Option<Instant>,
    frame: StageTiming,
    /// Longest stage of the last frame, and its share of the frame.
    critical: Option<(Stage, f32)>,
    measured_this_frame: [Option<f32>; 3],
}

impl FrameGraphTimeline {
    fn begin(&mut self, s: Stage) {
        self.open[s as usize] = Some(Instant::now());
    }

    fn end(&mut self, s: Stage) {
        if let Some(t0) = self.open[s as usize].take() {
            let ms = t0.elapsed().as_secs_f32() * 1000.0;
            self.stages[s as usize].record(ms);
            self.measured_this_frame[s as usize] = Some(ms);
        }
    }

    /// Close the frame: its total, and the stage that dominated it.
    pub fn close_frame(&mut self, total_ms: f32) {
        self.frame.record(total_ms);
        self.critical = longest_stage(&self.measured_this_frame).map(|(s, ms)| (s, if total_ms > 0.0 { ms / total_ms } else { 0.0 }));
        self.measured_this_frame = [None; 3];
    }

    /// The inspector's view.
    pub fn report(&self) -> FrameGraphReport {
        FrameGraphReport {
            frame: self.frame,
            stages: STAGE_NAMES.iter().zip(self.stages.iter()).map(|(n, t)| ((*n).to_string(), *t)).collect(),
            critical_stage: self.critical.map(|(s, _)| STAGE_NAMES[s as usize].to_string()),
            critical_share: self.critical.map(|(_, f)| f),
        }
    }
}

/// The stage that took longest this frame, if any was measured.
pub fn longest_stage(measured: &[Option<f32>; 3]) -> Option<(Stage, f32)> {
    let stages = [Stage::Physics, Stage::WorldSim, Stage::Play];
    stages
        .iter()
        .zip(measured.iter())
        .filter_map(|(s, m)| m.map(|ms| (*s, ms)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// What the timeline shows the inspector.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FrameGraphReport {
    pub frame: StageTiming,
    pub stages: Vec<(String, StageTiming)>,
    pub critical_stage: Option<String>,
    pub critical_share: Option<f32>,
}

fn frame_open(mut t: ResMut<FrameGraphTimeline>) {
    t.frame_start = Some(Instant::now());
}
fn frame_close(mut t: ResMut<FrameGraphTimeline>) {
    if let Some(t0) = t.frame_start.take() {
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        t.close_frame(ms);
    }
}
fn physics_open(mut t: ResMut<FrameGraphTimeline>) {
    t.begin(Stage::Physics);
}
fn physics_close(mut t: ResMut<FrameGraphTimeline>) {
    t.end(Stage::Physics);
}
fn world_sim_open(mut t: ResMut<FrameGraphTimeline>) {
    t.begin(Stage::WorldSim);
}
fn world_sim_close(mut t: ResMut<FrameGraphTimeline>) {
    t.end(Stage::WorldSim);
}
fn play_open(mut t: ResMut<FrameGraphTimeline>) {
    t.begin(Stage::Play);
}
fn play_close(mut t: ResMut<FrameGraphTimeline>) {
    t.end(Stage::Play);
}

/// Register the timeline's markers. While no session runs the Play sets are
/// skipped, so the play stage reads as the near-zero gap between its two
/// markers.
pub fn add_frame_graph_markers(app: &mut App) {
    app.init_resource::<FrameGraphTimeline>()
        .add_systems(First, frame_open)
        .add_systems(Last, frame_close)
        .add_systems(RunFixedMainLoop, physics_open.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop))
        .add_systems(RunFixedMainLoop, physics_close.in_set(RunFixedMainLoopSystems::AfterFixedMainLoop))
        .add_systems(Update, world_sim_open.before(WorldSimSet::Predict))
        .add_systems(Update, world_sim_close.after(WorldSimSet::Tier))
        .add_systems(Update, play_open.before(PlayScriptSet::Pull))
        .add_systems(Update, play_close.after(PlayScriptSet::End));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rage_stage_timing_averages_and_keeps_the_max() {
        let mut t = StageTiming::default();
        t.record(10.0);
        assert_eq!((t.last_ms, t.avg_ms, t.max_ms), (10.0, 10.0, 10.0));
        t.record(20.0);
        assert!((t.avg_ms - 11.0).abs() < 1e-5);
        assert_eq!(t.max_ms, 20.0);
        t.record(f32::NAN);
        assert_eq!(t.samples, 2, "a bad sample is dropped");
    }

    #[test]
    fn rage_critical_path_is_the_longest_measured_stage() {
        assert_eq!(longest_stage(&[None, None, None]), None);
        let got = longest_stage(&[Some(4.0), Some(0.5), Some(7.5)]);
        assert_eq!(got, Some((Stage::Play, 7.5)));
        let mut tl = FrameGraphTimeline::default();
        tl.measured_this_frame = [Some(6.0), Some(1.0), None];
        tl.close_frame(12.0);
        let r = tl.report();
        assert_eq!(r.critical_stage.as_deref(), Some("physics"));
        assert_eq!(r.critical_share, Some(0.5));
        assert_eq!(tl.measured_this_frame, [None; 3], "the next frame starts clean");
    }

    #[test]
    fn rage_world_sim_sets_run_in_order() {
        #[derive(Resource, Default)]
        struct Log(Vec<&'static str>);
        let mut app = App::new();
        app.init_resource::<Log>();
        configure_world_sim_sets(&mut app);
        app.add_systems(Update, (|mut l: ResMut<Log>| l.0.push("tier")).in_set(WorldSimSet::Tier));
        app.add_systems(Update, (|mut l: ResMut<Log>| l.0.push("stream")).in_set(WorldSimSet::Stream));
        app.add_systems(Update, (|mut l: ResMut<Log>| l.0.push("predict")).in_set(WorldSimSet::Predict));
        app.update();
        assert_eq!(app.world().resource::<Log>().0, vec!["predict", "stream", "tier"]);
    }
}
