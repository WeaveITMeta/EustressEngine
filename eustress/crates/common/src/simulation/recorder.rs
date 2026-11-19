//! # Data Recorder
//!
//! Time-series data collection and export for simulation analysis.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// A complete simulation recording
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimulationRecording {
    /// Recording metadata
    pub metadata: RecordingMetadata,
    
    /// Time series data by variable name
    pub series: HashMap<String, TimeSeries>,
    
    /// Events that occurred during simulation
    pub events: Vec<SimulationEvent>,
}

/// Metadata about the recording
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecordingMetadata {
    /// Recording name/identifier
    pub name: String,
    
    /// When recording started (wall clock)
    pub started_at: String,
    
    /// Total simulation time covered
    pub simulation_duration_s: f64,
    
    /// Total wall time elapsed
    pub wall_duration_s: f64,
    
    /// Total ticks recorded
    pub total_ticks: u64,
    
    /// Effective time compression ratio
    pub compression_ratio: f64,
    
    /// Additional metadata
    pub tags: HashMap<String, String>,

    /// Physics steps executed against the steps the clock's time implies.
    /// `None` in recordings written before the step counter existed.
    #[serde(default)]
    pub step_count: Option<StepCountReport>,
}

impl Default for RecordingMetadata {
    fn default() -> Self {
        Self {
            name: "Untitled Recording".to_string(),
            started_at: String::new(),
            simulation_duration_s: 0.0,
            wall_duration_s: 0.0,
            total_ticks: 0,
            compression_ratio: 1.0,
            tags: HashMap::new(),
            step_count: None,
        }
    }
}

/// One edge of a run's step-count window: the step counter, the clock and
/// the fixed-timestep remainder, all read at the same instant.
///
/// The engine takes a sample every Playing frame just before the clock
/// advances (in `PreUpdate`). At that instant the clock holds every advance
/// up to the previous frame and the counter holds every physics step up to
/// the previous frame's fixed loop, so the clock and physics have covered
/// the same frames. The run's first sample opens the window; its last sample
/// closes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StepWindowSample {
    /// [`PhysicsStepCounter::executed`] at the sample.
    ///
    /// [`PhysicsStepCounter::executed`]: super::clock::PhysicsStepCounter::executed
    pub physics_step_counter: u64,
    /// [`SimulationClock::tick_count`] at the sample.
    ///
    /// [`SimulationClock::tick_count`]: super::clock::SimulationClock::tick_count
    pub clock_tick_count: u64,
    /// The clock's simulated seconds at the sample.
    pub simulation_time_s: f64,
    /// The clock's wall seconds at the sample.
    pub wall_time_s: f64,
    /// Time the fixed-timestep loop held but had not yet stepped (Bevy's
    /// `Time<Fixed>::overstep()`, seconds) at the sample.
    pub fixed_overstep_s: f64,
}

impl StepWindowSample {
    /// Read one sample from the counter's value, the clock and the fixed
    /// loop's overstep.
    pub fn new(physics_step_counter: u64, clock: &super::clock::SimulationClock, fixed_overstep_s: f64) -> Self {
        Self {
            physics_step_counter,
            clock_tick_count: clock.tick_count,
            simulation_time_s: clock.simulation_time_s,
            wall_time_s: clock.wall_time_s,
            fixed_overstep_s,
        }
    }
}

/// One run's physics-step accounting, exported inside the recording and as a
/// standalone file a capture manifest can cite.
///
/// The physics and clock counts cover one window: from the clock's first
/// advance of the run to the last frame whose physics ran in Play (see
/// [`StepWindowSample`]). Physics steps outside that window are reported in
/// their own fields and are not counted in it:
/// - `physics_steps_before_clock_start`: steps physics executed between
///   entering Play and the clock's first advance (the frame that enters Play
///   steps physics, and the clock first advances in the next frame).
/// - `clock_ticks_after_physics_end`: ticks the clock drained after the last
///   frame whose physics ran (the frame that leaves Play advances the clock
///   before the state changes, and physics does not step in it).
///
/// Three counts over the window, from three sources:
/// - `physics_steps_executed`: the [`PhysicsStepCounter`] at the window's end
///   minus at its start; incremented by the physics schedule per step run.
/// - `clock_tick_count`: [`SimulationClock::tick_count`] at the window's end
///   minus at its start, the timesteps the clock's accumulator drained
///   (capped per frame).
/// - `clock_tick_count_implied_by_time`:
///   `round(simulation_time_s / fixed_timestep_s)`, the steps the clock's
///   time over the window stands for.
///
/// `dropped_steps` is implied minus executed. It is signed: a negative value
/// means physics stepped more than the clock's time accounts for, and that
/// is reported too rather than clamped to zero.
///
/// The fixed loop's overstep at each edge is exported as well. Over the
/// window, `physics_steps_executed * timestep` equals
/// `fixed_overstep_at_start_s + window time - fixed_overstep_at_end_s`, so
/// when frame deltas are not whole timesteps, a difference between the two
/// oversteps of more than half a step moves `dropped_steps` by one; the two
/// fields say by how much.
///
/// [`PhysicsStepCounter`]: super::clock::PhysicsStepCounter
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StepCountReport {
    /// Format tag for readers of the standalone file.
    pub schema: String,
    /// Physics steps executed over the window (counter end minus start).
    pub physics_steps_executed: u64,
    /// The monotonic counter's value when the window opened (the clock's
    /// first advance of the run).
    pub physics_step_counter_at_start: u64,
    /// The monotonic counter's value when the window closed.
    pub physics_step_counter_at_end: u64,
    /// Physics steps executed after entering Play and before the window
    /// opened. Not counted in `physics_steps_executed`.
    #[serde(default)]
    pub physics_steps_before_clock_start: u64,
    /// The clock's own drained-tick count over the window.
    pub clock_tick_count: u64,
    /// Ticks the clock drained after the window closed, in the frame that
    /// left Play. Not counted in `clock_tick_count`.
    #[serde(default)]
    pub clock_ticks_after_physics_end: u64,
    /// `round(simulation_time_s / fixed_timestep_s)`.
    pub clock_tick_count_implied_by_time: u64,
    /// `clock_tick_count_implied_by_time - physics_steps_executed`.
    pub dropped_steps: i64,
    /// The clock's simulated seconds over the window.
    pub simulation_time_s: f64,
    /// The clock's wall seconds over the window.
    pub wall_time_s: f64,
    /// The clock's fixed timestep (simulation seconds).
    pub fixed_timestep_s: f64,
    /// The clock's time scale at the run's end.
    pub time_scale: f64,
    /// The clock's per-frame tick cap at the run's end.
    pub max_ticks_per_frame: u32,
    /// The fixed loop's overstep (seconds) when the window opened.
    #[serde(default)]
    pub fixed_overstep_at_start_s: f64,
    /// The fixed loop's overstep (seconds) when the window closed.
    #[serde(default)]
    pub fixed_overstep_at_end_s: f64,
}

impl StepCountReport {
    /// Format tag written into `schema`. v2: the counts cover the window
    /// from the clock's first advance to the last frame physics ran in, and
    /// the steps and ticks outside it have their own fields.
    pub const SCHEMA: &'static str = "eustress.stepcount.v2";

    /// Build the report from the counter's value when the run entered Play,
    /// the window's two edges, and the clock as the run left it. Nothing here
    /// derives the executed count from the clock: it is the counter's
    /// difference only.
    pub fn new(
        counter_at_play_enter: u64,
        window_start: &StepWindowSample,
        window_end: &StepWindowSample,
        clock_at_stop: &super::clock::SimulationClock,
    ) -> Self {
        let physics_steps_executed = window_end
            .physics_step_counter
            .saturating_sub(window_start.physics_step_counter);
        let simulation_time_s = window_end.simulation_time_s - window_start.simulation_time_s;
        let implied = Self::ticks_implied_by_time(simulation_time_s, clock_at_stop.fixed_timestep_s);
        let dropped = (implied as i128 - physics_steps_executed as i128)
            .clamp(i64::MIN as i128, i64::MAX as i128) as i64;
        Self {
            schema: Self::SCHEMA.to_string(),
            physics_steps_executed,
            physics_step_counter_at_start: window_start.physics_step_counter,
            physics_step_counter_at_end: window_end.physics_step_counter,
            physics_steps_before_clock_start: window_start
                .physics_step_counter
                .saturating_sub(counter_at_play_enter),
            clock_tick_count: window_end.clock_tick_count.saturating_sub(window_start.clock_tick_count),
            clock_ticks_after_physics_end: clock_at_stop.tick_count.saturating_sub(window_end.clock_tick_count),
            clock_tick_count_implied_by_time: implied,
            dropped_steps: dropped,
            simulation_time_s,
            wall_time_s: window_end.wall_time_s - window_start.wall_time_s,
            fixed_timestep_s: clock_at_stop.fixed_timestep_s,
            time_scale: clock_at_stop.time_scale,
            max_ticks_per_frame: clock_at_stop.max_ticks_per_frame,
            fixed_overstep_at_start_s: window_start.fixed_overstep_s,
            fixed_overstep_at_end_s: window_end.fixed_overstep_s,
        }
    }

    /// `round(simulation_time_s / fixed_timestep_s)`, or 0 when either is
    /// not a positive finite number.
    pub fn ticks_implied_by_time(simulation_time_s: f64, fixed_timestep_s: f64) -> u64 {
        if !(fixed_timestep_s > 0.0) || !fixed_timestep_s.is_finite() || !simulation_time_s.is_finite() {
            return 0;
        }
        let ticks = (simulation_time_s / fixed_timestep_s).round();
        if ticks <= 0.0 { 0 } else { ticks as u64 }
    }

    /// Write the report as pretty JSON, creating the parent directory.
    pub fn export_json(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }
}

/// A time series of values
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeSeries {
    /// Variable name
    pub name: String,
    
    /// Display label
    pub label: String,
    
    /// Unit string
    pub unit: String,
    
    /// Time values (simulation seconds)
    pub times: Vec<f64>,
    
    /// Data values
    pub values: Vec<f64>,
    
    /// Statistics
    pub stats: TimeSeriesStats,
}

/// Statistics for a time series
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimeSeriesStats {
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub std_dev: f64,
    pub first: f64,
    pub last: f64,
}

impl TimeSeries {
    /// Create new time series
    pub fn new(name: &str, label: &str, unit: &str) -> Self {
        Self {
            name: name.to_string(),
            label: label.to_string(),
            unit: unit.to_string(),
            times: Vec::new(),
            values: Vec::new(),
            stats: TimeSeriesStats::default(),
        }
    }
    
    /// Add a data point
    pub fn push(&mut self, time: f64, value: f64) {
        self.times.push(time);
        self.values.push(value);
    }
    
    /// Calculate statistics
    pub fn compute_stats(&mut self) {
        if self.values.is_empty() {
            return;
        }
        
        let n = self.values.len() as f64;
        let sum: f64 = self.values.iter().sum();
        let mean = sum / n;
        
        let variance: f64 = self.values.iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f64>() / n;
        
        self.stats = TimeSeriesStats {
            min: self.values.iter().cloned().fold(f64::INFINITY, f64::min),
            max: self.values.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            mean,
            std_dev: variance.sqrt(),
            first: self.values.first().copied().unwrap_or(0.0),
            last: self.values.last().copied().unwrap_or(0.0),
        };
    }
}

/// An event that occurred during simulation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimulationEvent {
    /// Simulation time when event occurred
    pub time_s: f64,
    
    /// Tick when event occurred
    pub tick: u64,
    
    /// Event type/category
    pub event_type: String,
    
    /// Event description
    pub description: String,
    
    /// Associated data
    pub data: HashMap<String, f64>,
}

impl SimulationRecording {
    /// Create new recording
    pub fn new(name: &str) -> Self {
        Self {
            metadata: RecordingMetadata {
                name: name.to_string(),
                ..default()
            },
            series: HashMap::new(),
            events: Vec::new(),
        }
    }
    
    /// Add or update a time series
    pub fn add_series(&mut self, series: TimeSeries) {
        self.series.insert(series.name.clone(), series);
    }
    
    /// Record an event
    pub fn add_event(&mut self, event: SimulationEvent) {
        self.events.push(event);
    }
    
    /// Finalize recording and compute statistics
    pub fn finalize(&mut self) {
        for series in self.series.values_mut() {
            series.compute_stats();
        }
        
        if self.metadata.wall_duration_s > 0.0 {
            self.metadata.compression_ratio = 
                self.metadata.simulation_duration_s / self.metadata.wall_duration_s;
        }
    }
    
    /// Export to JSON file
    pub fn export_json(&self, path: &Path) -> std::io::Result<()> {
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }
    
    /// Export to CSV file (one file per series)
    pub fn export_csv(&self, directory: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(directory)?;
        
        for (name, series) in &self.series {
            let filename = format!("{}.csv", name.replace(['/', '\\', ':'], "_"));
            let path = directory.join(filename);
            let mut file = File::create(path)?;
            
            writeln!(file, "time_s,{}", name)?;
            for (t, v) in series.times.iter().zip(series.values.iter()) {
                writeln!(file, "{},{}", t, v)?;
            }
        }
        
        let events_path = directory.join("events.csv");
        let mut events_file = File::create(events_path)?;
        writeln!(events_file, "time_s,tick,event_type,description")?;
        for event in &self.events {
            writeln!(events_file, "{},{},{},\"{}\"", 
                event.time_s, event.tick, event.event_type, event.description)?;
        }
        
        Ok(())
    }
    
    /// Generate summary report as string
    pub fn summary(&self) -> String {
        let mut report = String::new();
        
        report.push_str(&format!("# Simulation Report: {}\n\n", self.metadata.name));
        report.push_str("## Summary\n");
        report.push_str(&format!("- Simulation Duration: {:.2} s ({:.2} hours)\n", 
            self.metadata.simulation_duration_s,
            self.metadata.simulation_duration_s / 3600.0));
        report.push_str(&format!("- Wall Time: {:.2} s\n", self.metadata.wall_duration_s));
        report.push_str(&format!("- Compression Ratio: {:.0}x\n", self.metadata.compression_ratio));
        report.push_str(&format!("- Total Ticks: {}\n\n", self.metadata.total_ticks));
        if let Some(steps) = &self.metadata.step_count {
            report.push_str(&format!(
                "- Physics Steps Executed: {} (clock time implies {}, dropped {}; {} before the clock's first advance, {} clock ticks after the last physics frame, not counted)\n\n",
                steps.physics_steps_executed,
                steps.clock_tick_count_implied_by_time,
                steps.dropped_steps,
                steps.physics_steps_before_clock_start,
                steps.clock_ticks_after_physics_end,
            ));
        }

        report.push_str("## Variables\n");
        for (name, series) in &self.series {
            report.push_str(&format!("### {} ({})\n", series.label, series.unit));
            report.push_str(&format!("- Min: {:.4}\n", series.stats.min));
            report.push_str(&format!("- Max: {:.4}\n", series.stats.max));
            report.push_str(&format!("- Mean: {:.4}\n", series.stats.mean));
            report.push_str(&format!("- Std Dev: {:.4}\n", series.stats.std_dev));
            report.push_str(&format!("- Start → End: {:.4} → {:.4}\n\n", 
                series.stats.first, series.stats.last));
        }
        
        if !self.events.is_empty() {
            report.push_str("## Events\n");
            for event in &self.events {
                report.push_str(&format!("- [{:.2}s] {}: {}\n", 
                    event.time_s, event.event_type, event.description));
            }
        }
        
        report
    }
}

fn default<T: Default>() -> T {
    T::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulation::clock::SimulationClock;

    /// The engine's frame order, one Playing frame at a time: the window
    /// sample, then the clock's advance, then one physics step (Avian's
    /// 60 Hz fixed loop at a 60 Hz frame). The frame that enters Play steps
    /// physics before any of this; the frame that leaves Play samples and
    /// advances the clock, and physics does not step in it.
    fn run(time_scale: f64, max_ticks_per_frame: u32, play_enter: u64, frames: u32) -> StepCountReport {
        let mut clock = SimulationClock::new(time_scale);
        clock.max_ticks_per_frame = max_ticks_per_frame;
        // Frame N enters Play: physics steps once, the clock does not advance.
        let mut counter = play_enter + 1;
        let mut start = None;
        for _ in 0..frames {
            start.get_or_insert(StepWindowSample::new(counter, &clock, 0.0));
            clock.advance(clock.fixed_timestep_s);
            counter += 1;
        }
        // The frame that leaves Play: sampled, the clock advances, no step.
        let end = StepWindowSample::new(counter, &clock, 0.0);
        clock.advance(clock.fixed_timestep_s);
        StepCountReport::new(play_enter, &start.expect("at least one frame"), &end, &clock)
    }

    /// Unsaturated at scale 1: one step per drained tick, nothing dropped.
    /// The physics step of the frame that enters Play and the clock tick of
    /// the frame that leaves it are reported in their own fields; counting
    /// either inside the window gives -1 or +1 here.
    #[test]
    fn unsaturated_run_drops_nothing() {
        let report = run(1.0, 10, 40, 600);
        assert_eq!(report.physics_steps_before_clock_start, 1);
        assert_eq!(report.physics_step_counter_at_start, 41);
        assert_eq!(report.physics_steps_executed, 600);
        assert_eq!(report.clock_tick_count, 600);
        assert_eq!(report.clock_ticks_after_physics_end, 1);
        assert_eq!(report.clock_tick_count_implied_by_time, 600);
        assert_eq!(report.dropped_steps, 0);
    }

    /// Saturated: the clock's time runs ahead of the steps physics executed,
    /// and the report says by how much.
    #[test]
    fn saturated_run_reports_the_drop() {
        let report = run(8.0, 2, 0, 300);
        assert_eq!(report.physics_steps_before_clock_start, 1);
        assert_eq!(report.physics_steps_executed, 300);
        assert_eq!(report.clock_tick_count, 600);
        assert_eq!(report.clock_ticks_after_physics_end, 2);
        assert_eq!(report.clock_tick_count_implied_by_time, 2400);
        assert_eq!(report.dropped_steps, 2100);
        assert!(report.physics_steps_executed < report.clock_tick_count_implied_by_time);
    }

    /// The fixed loop's remainder at each edge is exported, and a difference
    /// of more than half a step between them shows in `dropped_steps` as a
    /// signed one, never clamped: 0.8 of a step carried in, 10.3 steps of
    /// clock time, 0.1 carried out is 11 executed steps against 10 implied.
    #[test]
    fn overstep_phase_is_exported_and_excess_steps_are_negative() {
        let clock_dt = SimulationClock::default().fixed_timestep_s;
        let mut clock = SimulationClock::new(1.0);
        let start = StepWindowSample::new(5, &clock, 0.8 * clock_dt);
        clock.advance(10.3 * clock_dt);
        let end = StepWindowSample::new(16, &clock, 0.1 * clock_dt);
        let report = StepCountReport::new(4, &start, &end, &clock);
        assert_eq!(report.physics_steps_executed, 11);
        assert_eq!(report.clock_tick_count_implied_by_time, 10);
        assert_eq!(report.dropped_steps, -1);
        assert_eq!(report.fixed_overstep_at_start_s, 0.8 * clock_dt);
        assert_eq!(report.fixed_overstep_at_end_s, 0.1 * clock_dt);
        assert_eq!(report.clock_ticks_after_physics_end, 0);
    }

    /// A v1 report, written before the window fields existed, still reads.
    #[test]
    fn v1_report_reads_without_window_fields() {
        let v1 = r#"{
            "schema": "eustress.stepcount.v1",
            "physics_steps_executed": 602,
            "physics_step_counter_at_start": 0,
            "physics_step_counter_at_end": 602,
            "clock_tick_count": 601,
            "clock_tick_count_implied_by_time": 601,
            "dropped_steps": -1,
            "simulation_time_s": 10.0,
            "wall_time_s": 10.0,
            "fixed_timestep_s": 0.016666666666666666,
            "time_scale": 1.0,
            "max_ticks_per_frame": 10
        }"#;
        let report: StepCountReport = serde_json::from_str(v1).expect("v1 report parses");
        assert_eq!(report.dropped_steps, -1);
        assert_eq!(report.physics_steps_before_clock_start, 0);
        assert_eq!(report.clock_ticks_after_physics_end, 0);
        assert_eq!(report.fixed_overstep_at_start_s, 0.0);
    }

    /// A recording written before the counter existed still reads.
    #[test]
    fn old_recordings_read_without_step_count() {
        let old = r#"{
            "metadata": {
                "name": "simulation_run",
                "started_at": "",
                "simulation_duration_s": 1.0,
                "wall_duration_s": 1.0,
                "total_ticks": 60,
                "compression_ratio": 1.0,
                "tags": {}
            },
            "series": {},
            "events": []
        }"#;
        let rec: SimulationRecording = serde_json::from_str(old).expect("old recording parses");
        assert!(rec.metadata.step_count.is_none());
    }
}
