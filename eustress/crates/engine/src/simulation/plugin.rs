//! # Simulation Plugin
//!
//! Core Bevy plugin for tick-based simulation with time compression.
//! Integrates with PlayModeState for proper play/pause/stop behavior.

use avian3d::prelude::{Physics, PhysicsSchedule, PhysicsStepSystems, PhysicsTime};
use bevy::prelude::*;
use tracing::warn;
use eustress_common::simulation::{
    SimulationClock, SimulationState, SimulationMode,
    WatchPointRegistry, BreakPointRegistry,
    SimulationRecording, TimeSeries, WatchPoint, BreakPoint, Comparison,
    PhysicsStepCounter, StepCountReport, StepWindowSample,
};

use crate::play_mode::{PlayModeState, StopPlayEvent, TogglePauseEvent};

use super::command::{drain_sim_command_queues, merged_sim_values, SimQueueDrains, SimRunLedger};
use super::ipc::{sync_sim_ipc, SimIpc};

/// Bevy Resource mirror of SIM_VALUES thread-local.
/// Written by `publish_echem_to_sim_values` (Update), read by `record_and_stream_watchpoints` (PostUpdate).
/// Avoids thread-local cross-thread visibility issues in Bevy's multi-threaded executor.
#[derive(Resource, Default)]
pub struct SimValuesResource(pub std::collections::HashMap<String, f64>);

/// Sim values written THIS FRAME by a script (`set_sim_value` from Rune) or by
/// the MCP `set_sim_value` command — as opposed to the much larger set the
/// engine publishes out of the ECS every frame.
///
/// The distinction is load-bearing. `apply_sim_values_to_ecs` maps sim values
/// onto `ElectrochemicalState`, and its `battery.mode` default is "discharge at
/// 0.5C" — which it re-applies every single frame. Without knowing that a
/// script explicitly wrote `battery.current`, that default silently overwrites
/// the write on the very next frame, and comparing against the ECS value can't
/// tell "script wrote 0.5" from "engine published 0.5" once the loop reaches
/// steady state. So the writer records its keys here and the consumer checks
/// them.
///
/// Replaced (not merged) once per frame by the Rune driver, so a key only
/// counts for the frame it was written in; stop writing and normal mode
/// behaviour resumes.
#[derive(Resource, Default)]
pub struct ScriptSimWrites(pub std::collections::HashMap<String, f64>);

/// Requested auto-stop target (simulation time in seconds).
/// Set by a `run` command that carries `duration_s`;
/// cleared on stop or when the threshold is crossed.
#[derive(Resource, Default)]
pub struct SimAutoStop {
    pub stop_at_sim_s: Option<f64>,
}

/// The current run's window on [`PhysicsStepCounter`], and where else its
/// step-count report goes.
///
/// `start` is the counter's value when the run entered Play; a resume from
/// Pause keeps it, and the stop takes it. The counter itself is never reset.
/// The report's counts cover [`StepCountWindow`], which opens one frame
/// later; `start` gives the steps executed between the two.
#[derive(Resource, Default, Debug)]
pub struct StepCountRun {
    /// The counter at the run's start; `None` between runs.
    pub start: Option<u64>,
    /// An extra path the report is written to on every stop, besides the
    /// `stepcount_*.json` next to the recording (a headless batch run's
    /// `--export-stepcount`).
    pub export_path: Option<std::path::PathBuf>,
}

/// The current run's step-count window: the frames the clock advanced in
/// whose physics also ran in Play.
///
/// Within a frame the clock advances in `PreUpdate`, before the state
/// transition and before the fixed loop that steps physics. So the frame
/// that enters Play steps physics with no clock advance (the state was not
/// Playing yet in its `PreUpdate`), and the frame that leaves Play advances
/// the clock with no physics step (the state has changed by its fixed loop).
/// `sample_step_window` samples every Playing frame just before the
/// advance: the first sample opens the window, the latest one is its end,
/// and neither edge frame is inside it. A resume from Pause keeps the start.
#[derive(Resource, Default, Debug)]
pub struct StepCountWindow {
    /// The sample at the clock's first advance of the run; `None` between
    /// runs.
    pub start: Option<StepWindowSample>,
    /// The latest sample of the run.
    pub end: Option<StepWindowSample>,
}

/// Open the run's step-count accounting on entering Play. A resume from
/// Pause enters Playing again and keeps the value the run opened with.
fn open_step_count_run(step_counter: Res<PhysicsStepCounter>, mut step_run: ResMut<StepCountRun>) {
    if step_run.start.is_none() {
        step_run.start = Some(step_counter.executed());
    }
}

/// Sample the counter, the clock and the fixed loop's overstep just before
/// the clock advances. Runs every Playing frame, whether or not the clock
/// then ticks, so a frame whose clock holds still while physics steps is
/// inside the window and shows.
fn sample_step_window(
    step_counter: Res<PhysicsStepCounter>,
    clock: Res<SimulationClock>,
    fixed: Option<Res<Time<Fixed>>>,
    mut window: ResMut<StepCountWindow>,
) {
    let overstep_s = fixed.map(|t| t.overstep().as_secs_f64()).unwrap_or(0.0);
    let sample = StepWindowSample::new(step_counter.executed(), &*clock, overstep_s);
    if window.start.is_none() {
        window.start = Some(sample);
    }
    window.end = Some(sample);
}

/// Close the run's step-count accounting and build its report. `now` is
/// read at the stop; it stands in for an edge only when the run never
/// reached a clock advance (then the window is empty).
pub(crate) fn close_step_count_run(
    step_run: &mut StepCountRun,
    window: &mut StepCountWindow,
    now: StepWindowSample,
    clock_at_stop: &SimulationClock,
) -> StepCountReport {
    let start = window.start.take().unwrap_or(now);
    let end = window.end.take().unwrap_or(start);
    let play_enter = step_run.start.take().unwrap_or(start.physics_step_counter);
    StepCountReport::new(play_enter, &start, &end, clock_at_stop)
}

/// Count every executed physics step into [`PhysicsStepCounter`].
///
/// The system sits inside Avian's `PhysicsSchedule`, which Avian's
/// `run_physics_schedule` runs once per fixed tick of `FixedPostUpdate`, and
/// only when the physics clock has a non-zero delta (not paused) and the
/// Kinematics domain allows the step. A frame that catches up several fixed
/// ticks runs the schedule, and so this system, once per tick. The clock's
/// `advance` loop is in a different schedule (`PreUpdate`) driven by a
/// different accumulator, so nothing here reads or follows the clock.
pub(crate) fn add_physics_step_counter(app: &mut App) {
    app.init_resource::<PhysicsStepCounter>()
        .add_systems(PhysicsSchedule, count_physics_step.in_set(PhysicsStepSystems::Last));
}

/// One run of `PhysicsSchedule` reached its last set: one step executed.
fn count_physics_step(mut counter: ResMut<PhysicsStepCounter>) {
    counter.record_step();
}

/// Stop Avian at the step it is on.
///
/// `pause()` alone lets one more step through. Avian's `run_physics_schedule`
/// runs the schedule whenever the physics clock's delta is non-zero; a paused
/// clock is not advanced, and it still carries the delta of its last step
/// until the run after the pause clears it. Clearing the delta here makes the
/// pause take effect before the next run, so a Pause, a Stop, the end of a
/// stepped run and a capture's hold all leave the world at the step they
/// were asked at.
pub fn hold_physics(physics: &mut Time<Physics>) {
    physics.pause();
    physics.advance_by(std::time::Duration::ZERO);
}

/// Core simulation plugin providing tick-based time compression
#[derive(Default)]
pub struct SimulationPlugin;

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        add_physics_step_counter(app);
        app.init_resource::<SimulationClock>()
            .init_resource::<StepCountRun>()
            .init_resource::<StepCountWindow>()
            .init_resource::<SimulationState>()
            .init_resource::<SimValuesResource>()
            .init_resource::<ScriptSimWrites>()
            .init_resource::<SimAutoStop>()
            .init_resource::<WatchPointRegistry>()
            .init_resource::<BreakPointRegistry>()
            .init_resource::<super::data_binding::DataBindingRegistry>()
            .init_resource::<ActiveRecording>()
            .init_resource::<TelemetryWriterState>()
            .init_resource::<SimIpc>()
            .init_resource::<SimRunLedger>()
            .init_resource::<SimQueueDrains>()
            .register_type::<SimulationClock>()
            .register_type::<SimulationState>()
            // Sync simulation state with play mode transitions
            .add_systems(OnEnter(PlayModeState::Playing), on_play_start)
            .add_systems(OnEnter(PlayModeState::Playing), open_step_count_run)
            .add_systems(OnEnter(PlayModeState::Playing), register_battery_watchpoints)
            .add_systems(OnEnter(PlayModeState::Paused), on_play_pause)
            .add_systems(OnEnter(PlayModeState::Editing), on_play_stop)
            // Resolve this instance's IPC paths when the Space changes, then
            // drain its command queues every frame, in any state, so `run`
            // works from Edit and `stop` from Play. Never on a stagger slot: a
            // `stop` that waited for its slot would let the run go on for up to
            // three more frames.
            .add_systems(PreUpdate, (sync_sim_ipc, drain_sim_command_queues).chain())
            // Sample the step-count window just before the clock advances
            // (see `StepCountWindow` for why at this point).
            .add_systems(
                PreUpdate,
                sample_step_window
                    .run_if(in_state(PlayModeState::Playing))
                    .after(drain_sim_command_queues)
                    .before(advance_simulation_clock),
            )
            // Advance simulation clock when playing
            .add_systems(
                PreUpdate,
                advance_simulation_clock
                    .run_if(in_state(PlayModeState::Playing))
                    .after(drain_sim_command_queues),
            )
            // Auto-stop when requested duration_s is reached
            .add_systems(
                PreUpdate,
                check_auto_stop
                    .run_if(in_state(PlayModeState::Playing))
                    .after(advance_simulation_clock),
            )
            // Data → Sim: drive bound parameters from Dataset columns. Runs
            // after the clock (so `ByTime` samples the current sim time) and in
            // PreUpdate so Update's `apply_sim_values_to_ecs` sees the value the
            // same frame it is written.
            .add_systems(
                PreUpdate,
                super::data_binding::advance_data_bindings
                    .run_if(in_state(PlayModeState::Playing))
                    .after(advance_simulation_clock),
            )
            // Record watchpoint values + publish to stream each frame
            .add_systems(
                PostUpdate,
                record_and_stream_watchpoints.run_if(in_state(PlayModeState::Playing)),
            )
            // Append to the Universe's telemetry.jsonl for tail_telemetry (1 Hz)
            .add_systems(
                PostUpdate,
                write_telemetry_log
                    .run_if(in_state(PlayModeState::Playing))
                    .after(record_and_stream_watchpoints),
            );
    }
}

/// Called when entering Playing state - ensure simulation is running, and
/// open a run in the ledger (a resume from Pause continues the current one).
fn on_play_start(
    mut sim_state: ResMut<SimulationState>,
    clock: Res<SimulationClock>,
    mut ledger: ResMut<SimRunLedger>,
    mut stop_writer: MessageWriter<StopPlayEvent>,
    mut pause_writer: MessageWriter<TogglePauseEvent>,
) {
    // Always ensure Running mode when entering play
    sim_state.mode = SimulationMode::Running;
    sim_state.completed = false;
    info!("🎮 Simulation started (mode=Running)");

    // A `stop` or `pause` that arrived while this run was still starting is
    // honoured now that there is a run to apply it to.
    let (stop_after_start, pause_after_start) = ledger.begin_run(&clock);
    if stop_after_start {
        ledger.set_stop_reason("stop_command");
        stop_writer.write(StopPlayEvent);
    } else if pause_after_start {
        pause_writer.write(TogglePauseEvent);
    }
}

/// Called when entering Paused state - pause simulation
fn on_play_pause(mut sim_state: ResMut<SimulationState>) {
    sim_state.pause();
    info!("⏸️ Simulation paused");
}

/// Called when entering Editing state - reset simulation
fn on_play_stop(
    mut sim_clock: ResMut<SimulationClock>,
    mut sim_state: ResMut<SimulationState>,
    mut sim_values: ResMut<SimValuesResource>,
    mut watchpoints: ResMut<WatchPointRegistry>,
    mut breakpoints: ResMut<BreakPointRegistry>,
    mut auto_stop: ResMut<SimAutoStop>,
    mut recording: ResMut<ActiveRecording>,
    mut ledger: ResMut<SimRunLedger>,
    ipc: Res<SimIpc>,
    mut output: Option<ResMut<crate::ui::slint_ui::OutputConsole>>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    step_counter: Res<PhysicsStepCounter>,
    mut step_run: ResMut<StepCountRun>,
    mut step_window: ResMut<StepCountWindow>,
    fixed_time: Option<Res<Time<Fixed>>>,
) {
    // Capture the run's outcome FIRST: everything below resets it. This is
    // the only moment the final values still exist, so a client waiting on
    // the run reads them from the ledger rather than from a snapshot written
    // after the store was cleared.
    let final_ticks = sim_clock.tick_count;
    let final_sim_s = sim_clock.simulation_time_s;
    let final_values = merged_sim_values(Some(&*sim_values), Some(&*watchpoints));
    let run_id = ledger.active_run_id();
    let mut exported_recording = None;

    // Close the run's window on the physics step counter and compare it with
    // the clock BEFORE the clock resets. The counter is only read here; it
    // keeps counting across runs.
    let now = StepWindowSample::new(
        step_counter.executed(),
        &*sim_clock,
        fixed_time.map(|t| t.overstep().as_secs_f64()).unwrap_or(0.0),
    );
    let step_report = close_step_count_run(&mut *step_run, &mut *step_window, now, &*sim_clock);
    info!(
        "🔢 Physics steps executed: {} (clock drained {}, clock time implies {}, dropped {}; not counted: {} steps before the clock's first advance, {} clock ticks after the last physics frame)",
        step_report.physics_steps_executed,
        step_report.clock_tick_count,
        step_report.clock_tick_count_implied_by_time,
        step_report.dropped_steps,
        step_report.physics_steps_before_clock_start,
        step_report.clock_ticks_after_physics_end,
    );
    if let Some(path) = step_run.export_path.clone() {
        match step_report.export_json(&path) {
            Ok(()) => info!("💾 Step count exported to {}", path.display()),
            Err(e) => warn!("Failed to export step count to {}: {}", path.display(), e),
        }
    }

    // Stop and auto-export recording BEFORE resetting clock (clock.reset() zeros tick_count)
    if recording.enabled {
        // Write final clock state into recording metadata before stopping
        if let Some(ref mut rec) = recording.recording {
            rec.metadata.total_ticks = sim_clock.tick_count;
            rec.metadata.simulation_duration_s = sim_clock.simulation_time_s;
            rec.metadata.wall_duration_s = sim_clock.wall_time_s;
            rec.metadata.step_count = Some(step_report.clone());
        }
        if let Some(rec) = recording.stop() {
            let ticks = rec.metadata.total_ticks;
            let sim_duration = rec.metadata.simulation_duration_s;
            let series_count = rec.series.len();
            info!("📊 Simulation recording stopped: {} ticks, {:.2}s simulated, {} watchpoints",
                ticks, sim_duration, series_count);

            // Auto-export to Universe knowledge/recordings/{space_name}/
            let recordings_dir = ipc.recordings_dir().unwrap_or_else(|| match space_root {
                Some(ref sr) => sr.0.join(".eustress").join("recordings"),
                None => crate::space::workspace_root().join(".eustress").join("recordings"),
            });
            {
                if let Err(e) = std::fs::create_dir_all(&recordings_dir) {
                    warn!("Failed to create recordings dir: {}", e);
                } else {
                    // Millisecond timestamp + run id + PID: two engines on the
                    // same Space (design variants) stopping in the same second
                    // used to write the same `sim_<seconds>.json`, and the
                    // second export silently replaced the first.
                    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S_%3f");
                    let run_tag = run_id.map(|id| format!("_run{id}")).unwrap_or_default();
                    let json_path = recordings_dir.join(format!(
                        "sim_{}{}_pid{}.json",
                        timestamp,
                        run_tag,
                        SimIpc::pid()
                    ));
                    match rec.export_json(&json_path) {
                        Ok(_) => {
                            let msg = format!("Recording exported to {}", json_path.display());
                            info!("💾 {}", msg);
                            if let Some(ref mut out) = output {
                                out.info(msg);
                            }
                            exported_recording = Some(json_path.clone());
                        }
                        Err(e) => {
                            warn!("Failed to export recording: {}", e);
                            if let Some(ref mut out) = output {
                                out.error(format!("Failed to export recording: {}", e));
                            }
                        }
                    }
                    // The same step-count report as a standalone file beside
                    // the recording, named to match it, for a capture
                    // manifest to cite without parsing the recording.
                    let stepcount_path = recordings_dir.join(format!(
                        "stepcount_{}{}_pid{}.json",
                        timestamp,
                        run_tag,
                        SimIpc::pid()
                    ));
                    if let Err(e) = step_report.export_json(&stepcount_path) {
                        warn!("Failed to export step count: {}", e);
                    }
                    // Print summary to output panel
                    let summary = rec.summary();
                    info!("{}", summary);
                    if let Some(ref mut out) = output {
                        out.info(format!("Simulation: {} ticks, {:.2}s, {} watchpoints",
                            ticks, sim_duration, series_count));
                    }
                }
            }
        }
    }

    ledger.complete_run(final_ticks, final_sim_s, exported_recording, final_values);

    // Reset AFTER recording is saved — so tick_count and sim_time are preserved in the export
    sim_clock.reset();
    sim_state.reset();
    watchpoints.reset_all();
    breakpoints.reset_all();
    auto_stop.stop_at_sim_s = None;

    // Clear sim values so HUD and BillboardGui widgets return to their
    // TOML-initialized defaults rather than showing stale mid-run readings.
    sim_values.0.clear();

    // Also clear the thread-local mirror so Rune scripts on next on_init see clean state.
    crate::soul::rune_ecs_module::SIM_VALUES.with(|sv| sv.borrow_mut().clear());

    info!("⏹ Simulation stopped and reset");
}

/// System to advance simulation clock each frame
fn advance_simulation_clock(
    time: Res<Time>,
    mut clock: ResMut<SimulationClock>,
    mut state: ResMut<SimulationState>,
) {
    if !state.should_tick() {
        return;
    }

    let wall_delta = time.delta_secs_f64();
    let ticks_to_run = clock.advance(wall_delta);

    // Log every ~60 frames
    static TICK_LOG: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let log_frame = TICK_LOG.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if log_frame % 60 == 0 {
        info!("⏱ Sim clock: ticks_to_run={}, total_ticks={}, sim_time={:.2}s, wall_delta={:.4}s",
            ticks_to_run, clock.tick_count, clock.simulation_time_s, wall_delta);
    }
    
    for _ in 0..ticks_to_run {
        let should_continue = state.after_tick(
            clock.simulation_time_s,
            clock.tick_count,
        );
        
        if !should_continue {
            break;
        }
    }
}

/// Stop simulation when the requested duration is reached.
fn check_auto_stop(
    clock: Res<SimulationClock>,
    mut auto_stop: ResMut<SimAutoStop>,
    mut ledger: ResMut<SimRunLedger>,
    mut stop_play_writer: MessageWriter<StopPlayEvent>,
) {
    if let Some(stop_at) = auto_stop.stop_at_sim_s {
        if clock.simulation_time_s >= stop_at {
            info!("⏹ Auto-stop: sim time {:.3}s reached target {:.3}s", clock.simulation_time_s, stop_at);
            auto_stop.stop_at_sim_s = None;
            ledger.set_stop_reason("duration_reached");
            // Send the Stop MESSAGE rather than setting the state. Setting
            // `Editing` directly skipped `handle_stop_play` — the system that
            // actually restores transforms and despawns play-spawned entities
            // — so a duration-limited run ended with parts left wherever
            // physics dropped them.
            stop_play_writer.write(StopPlayEvent);
        }
    }
}

/// Active recording resource
#[derive(Resource, Default)]
pub struct ActiveRecording {
    /// Current recording if active
    pub recording: Option<SimulationRecording>,
    
    /// Whether recording is enabled
    pub enabled: bool,
}

impl ActiveRecording {
    /// Start a new recording
    pub fn start(&mut self, name: &str) {
        self.recording = Some(SimulationRecording::new(name));
        self.enabled = true;
    }
    
    /// Stop and finalize recording
    pub fn stop(&mut self) -> Option<SimulationRecording> {
        self.enabled = false;
        self.recording.take().map(|mut r| {
            r.finalize();
            r
        })
    }
}

/// Helper functions for simulation control from systems
pub fn pause_simulation(state: &mut SimulationState) {
    state.pause();
}

pub fn resume_simulation(state: &mut SimulationState) {
    state.resume();
}

pub fn step_simulation(state: &mut SimulationState) {
    state.step();
}

pub fn set_time_scale(clock: &mut SimulationClock, scale: f64) {
    clock.set_time_scale(scale);
}

pub fn reset_simulation(clock: &mut SimulationClock, state: &mut SimulationState) {
    clock.reset();
    state.reset();
}

/// Register a watchpoint for tracking
pub fn register_watchpoint(
    registry: &mut WatchPointRegistry,
    name: &str,
    label: &str,
    unit: &str,
) {
    registry.register(WatchPoint::new(name, label, unit));
}

/// Register a breakpoint for conditional pause
pub fn register_breakpoint(
    registry: &mut BreakPointRegistry,
    name: &str,
    variable: &str,
    comparison: &str,
    threshold: f64,
) {
    if let Some(comp) = Comparison::from_str(comparison) {
        registry.register(BreakPoint::new(name, variable, comp, threshold));
    }
}

// ============================================================================
// Battery Watchpoint Registration — auto-register for V-Cell demo
// ============================================================================

/// Register default watchpoints for the battery simulation demo.
/// Called on OnEnter(PlayModeState::Playing).
fn register_battery_watchpoints(
    mut watchpoints: ResMut<WatchPointRegistry>,
    mut recording: ResMut<ActiveRecording>,
) {
    // Register standard battery watchpoints if not already present
    let battery_watchpoints = [
        ("battery.voltage", "Cell Voltage", "V"),
        ("battery.current", "Current", "A"),
        ("battery.soc", "State of Charge", "%"),
        ("battery.temperature_c", "Temperature", "°C"),
        ("battery.power", "Power", "W"),
        ("battery.c_rate", "C-Rate", "C"),
        ("battery.dendrite_risk", "Dendrite Risk", "%"),
        ("battery.capacity_retention", "Capacity Retention", "%"),
        ("battery.cycle_count", "Cycle Count", ""),
    ];

    for (name, label, unit) in &battery_watchpoints {
        if watchpoints.get(name).is_none() {
            watchpoints.register(WatchPoint::new(name, label, unit));
        }
    }

    // Start recording automatically — once per run. `OnEnter(Playing)` also
    // fires on every resume from Pause, and restarting here threw away
    // everything recorded before the pause.
    if !recording.enabled {
        recording.start("simulation_run");
        info!("📊 Registered {} battery watchpoints, recording started", battery_watchpoints.len());
    }
}

// ============================================================================
// Watchpoint Recording + Stream Publishing — runs each frame during play
// ============================================================================

/// System: read SIM_VALUES, record to watchpoints, publish to EustressStream.
/// Runs in PostUpdate so it captures values AFTER script execution.
fn record_and_stream_watchpoints(
    clock: Res<SimulationClock>,
    mut watchpoints: ResMut<WatchPointRegistry>,
    mut recording: ResMut<ActiveRecording>,
    mut breakpoints: ResMut<BreakPointRegistry>,
    mut sim_state: ResMut<SimulationState>,
    sim_values_res: Res<SimValuesResource>,
    #[cfg(feature = "streaming")]
    change_queue: Option<Res<eustress_common::change_queue::ChangeQueue>>,
) {
    let sim_time = clock.simulation_time_s;
    let tick = clock.tick_count;

    // Read from Bevy Resource (cross-thread safe, populated by publish_echem_to_sim_values)
    let sim_values = &sim_values_res.0;

    if sim_values.is_empty() {
        return;
    }

    // Record each value into its watchpoint
    for (key, value) in sim_values.iter() {
        watchpoints.record(key, *value, sim_time, tick);

        // Also feed into active recording time series
        if recording.enabled {
            if let Some(ref mut rec) = recording.recording {
                if !rec.series.contains_key(key) {
                    let wp = watchpoints.get(key);
                    let label = wp.map(|w| w.label.as_str()).unwrap_or(key);
                    let unit = wp.map(|w| w.unit.as_str()).unwrap_or("");
                    rec.add_series(eustress_common::simulation::TimeSeries::new(key, label, unit));
                }
                if let Some(series) = rec.series.get_mut(key) {
                    series.push(sim_time, *value);
                }
            }
        }
    }

    // Check breakpoints
    let triggered = breakpoints.check_all(sim_values);
    for bp_name in &triggered {
        info!("🛑 Breakpoint '{}' triggered at tick {} (sim_time={:.2}s)", bp_name, tick, sim_time);
        sim_state.hit_breakpoint(bp_name);

        // Record breakpoint event in active recording
        if recording.enabled {
            if let Some(ref mut rec) = recording.recording {
                let mut data: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
                for (k, v) in sim_values.iter() {
                    data.insert(k.clone(), *v);
                }
                rec.add_event(eustress_common::simulation::SimulationEvent {
                    time_s: sim_time,
                    tick,
                    event_type: "breakpoint".to_string(),
                    description: format!("Breakpoint '{}' triggered", bp_name),
                    data,
                });
            }
        }

        // Publish breakpoint event to stream
        #[cfg(feature = "streaming")]
        {
            if let Some(ref cq) = change_queue {
                let payload = serde_json::json!({
                    "event": "breakpoint",
                    "breakpoint": bp_name,
                    "tick": tick,
                    "sim_time_s": sim_time,
                    "values": sim_values,
                });
                if let Ok(bytes) = serde_json::to_vec(&payload) {
                    cq.stream.producer(eustress_common::scene_delta::TOPIC_SIM_WATCHPOINTS)
                        .send_bytes(bytes::Bytes::from(bytes));
                }
            }
        }
    }

    // Publish watchpoint values to EustressStream (if streaming feature enabled)
    #[cfg(feature = "streaming")]
    {
        // Publish every 10th tick to avoid flooding the stream
        if tick % 10 == 0 {
            if let Some(ref cq) = change_queue {
                let payload = serde_json::json!({
                    "event": "tick",
                    "tick": tick,
                    "sim_time_s": sim_time,
                    "values": sim_values,
                });
                if let Ok(bytes) = serde_json::to_vec(&payload) {
                    cq.stream.producer(eustress_common::scene_delta::TOPIC_SIM_WATCHPOINTS)
                        .send_bytes(bytes::Bytes::from(bytes));
                }
            }
        }
    }
}

// ============================================================================
// Telemetry Writer — appends to telemetry.jsonl for the tail_telemetry tool
// ============================================================================

/// Throttle state for the telemetry log writer.
#[derive(Resource)]
pub struct TelemetryWriterState {
    last_write: std::time::Instant,
    interval: std::time::Duration,
}

impl Default for TelemetryWriterState {
    fn default() -> Self {
        Self {
            last_write: std::time::Instant::now() - std::time::Duration::from_secs(2),
            interval: std::time::Duration::from_secs(1), // 1 Hz
        }
    }
}

/// Append one JSONL line per second to `<universe>/.eustress/telemetry.jsonl`.
///
/// Each line: `{ "t": "<rfc3339>", "pid": u32, "space": "...", "run_id": u64,
/// "tick": u64, "sim_time_s": f64, "values": { "key": f64, ... } }`
///
/// The file is shared by every engine with a Space of this Universe open, so
/// each line names the instance, Space and run that wrote it (`tail_telemetry`
/// filters on them), and is appended with a single write: a line built by
/// several small writes can interleave with another engine's line mid-way.
///
/// The file grows unbounded (acceptable for alpha — a future
/// rotation/compaction system will cap it at ~10 MB).
fn write_telemetry_log(
    mut state: ResMut<TelemetryWriterState>,
    sim_values_res: Res<SimValuesResource>,
    clock: Res<SimulationClock>,
    ledger: Res<SimRunLedger>,
    ipc: Res<SimIpc>,
) {
    if state.last_write.elapsed() < state.interval { return }

    let sim_values = &sim_values_res.0;
    if sim_values.is_empty() { return }
    let Some(path) = ipc.telemetry() else { return };

    let entry = serde_json::json!({
        "t": chrono::Utc::now().to_rfc3339(),
        "pid": SimIpc::pid(),
        "space": ipc.space_name,
        "run_id": ledger.active_run_id(),
        "tick": clock.tick_count,
        "sim_time_s": clock.simulation_time_s,
        "values": sim_values,
    });

    if let Err(e) = eustress_bridge_client::append_json_line(&path, &entry) {
        warn!("Failed to write telemetry log: {}", e);
    }
    state.last_write = std::time::Instant::now();
}

#[cfg(test)]
mod tests {
    use super::*;
    use avian3d::prelude::{Physics, PhysicsPlugins, PhysicsTime};
    use std::time::Duration;

    /// Real Avian on a manual clock: every `update` advances time by `frame`.
    fn physics_app(frame: Duration) -> App {
        let mut app = App::new();
        bevy::tasks::IoTaskPool::get_or_init(Default::default);
        bevy::tasks::AsyncComputeTaskPool::get_or_init(Default::default);
        bevy::tasks::ComputeTaskPool::get_or_init(Default::default);
        app.add_plugins((
            bevy::time::TimePlugin,
            bevy::transform::TransformPlugin,
            bevy::asset::AssetPlugin::default(),
            bevy::diagnostic::DiagnosticsPlugin,
        ));
        app.init_resource::<avian3d::spatial_query::SpatialQueryDiagnostics>();
        app.init_resource::<avian3d::collider_tree::ColliderTreeDiagnostics>();
        app.init_resource::<avian3d::collision::CollisionDiagnostics>();
        app.init_resource::<avian3d::dynamics::solver::SolverDiagnostics>();
        app.init_asset::<Mesh>();
        app.insert_resource(Time::<Fixed>::from_hz(60.0));
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(frame));
        app.add_plugins(PhysicsPlugins::default());
        add_physics_step_counter(&mut app);
        app
    }

    fn executed(app: &App) -> u64 {
        app.world().resource::<PhysicsStepCounter>().executed()
    }

    /// A frame that owes two fixed ticks runs the physics schedule twice, and
    /// the counter counts both; a held physics clock steps nothing and
    /// counts nothing, and unpausing picks up from where it stopped.
    #[test]
    fn counts_every_step_of_a_catch_up_frame_and_none_while_paused() {
        let step = Time::<Fixed>::from_hz(60.0).timestep();
        let mut app = physics_app(step * 2);
        // Warm-up: the first frames settle the clocks.
        app.update();
        app.update();

        let before = executed(&app);
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(executed(&app) - before, 10, "two steps a frame for five frames");

        hold_physics(&mut app.world_mut().resource_mut::<Time<Physics>>());
        let paused_at = executed(&app);
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(executed(&app), paused_at, "a held physics clock executes no step");

        app.world_mut().resource_mut::<Time<Physics>>().unpause();
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(executed(&app), paused_at + 6, "the counter resumes from its value, never reset");
    }

    /// Why the engine holds physics with `hold_physics`: Avian's bare
    /// `pause()` lets exactly one more step through, and the hold lets none.
    #[test]
    fn a_bare_pause_lets_one_more_step_through_and_a_hold_does_not() {
        let step = Time::<Fixed>::from_hz(60.0).timestep();
        let mut app = physics_app(step);
        app.update();
        app.update();
        for _ in 0..5 {
            app.update();
        }

        app.world_mut().resource_mut::<Time<Physics>>().pause();
        let paused_at = executed(&app);
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(executed(&app), paused_at + 1, "Avian steps once more after a bare pause");

        app.world_mut().resource_mut::<Time<Physics>>().unpause();
        for _ in 0..5 {
            app.update();
        }
        hold_physics(&mut app.world_mut().resource_mut::<Time<Physics>>());
        let held_at = executed(&app);
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(executed(&app), held_at, "a held physics clock executes no step");
    }

    /// The counter does not follow the simulation clock: compressing and
    /// capping the clock changes its tick count, not the steps physics ran.
    #[test]
    fn counter_disagrees_with_a_saturated_clock() {
        let step = Time::<Fixed>::from_hz(60.0).timestep();
        let mut app = physics_app(step);
        app.update();
        app.update();

        let mut clock = SimulationClock::new(8.0);
        clock.max_ticks_per_frame = 2;
        let start = StepWindowSample::new(executed(&app), &clock, 0.0);
        for _ in 0..30 {
            app.update();
            clock.advance(step.as_secs_f64());
        }
        let end = StepWindowSample::new(executed(&app), &clock, 0.0);
        let report = StepCountReport::new(start.physics_step_counter, &start, &end, &clock);
        assert_eq!(report.physics_steps_executed, 30);
        assert_eq!(report.clock_tick_count, 60);
        assert_eq!(report.clock_tick_count_implied_by_time, 240);
        assert_eq!(report.dropped_steps, 210);
    }

    /// Reports from the test's stops, in order.
    #[derive(Resource, Default)]
    struct StopReports(Vec<StepCountReport>);

    /// What play mode does to physics on entering Play.
    fn unpause_physics(mut physics: ResMut<Time<Physics>>) {
        physics.unpause();
    }

    /// What `handle_stop_play` and `on_play_stop` do to physics, the window
    /// and the clock on the stop.
    fn stop_run(
        mut physics: ResMut<Time<Physics>>,
        counter: Res<PhysicsStepCounter>,
        fixed: Res<Time<Fixed>>,
        mut clock: ResMut<SimulationClock>,
        mut step_run: ResMut<StepCountRun>,
        mut window: ResMut<StepCountWindow>,
        mut reports: ResMut<StopReports>,
    ) {
        hold_physics(&mut physics);
        let now = StepWindowSample::new(counter.executed(), &*clock, fixed.overstep().as_secs_f64());
        reports.0.push(close_step_count_run(&mut *step_run, &mut *window, now, &*clock));
        clock.reset();
    }

    /// Real Avian and the real play-state schedules, with this plugin's
    /// window systems (`open_step_count_run`, `sample_step_window`,
    /// `advance_simulation_clock`, `close_step_count_run`) in their
    /// production places. Physics is paused in Edit, as play mode leaves it.
    fn play_app(frame: Duration) -> App {
        let mut app = physics_app(frame);
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<PlayModeState>();
        app.init_resource::<SimulationClock>()
            .init_resource::<SimulationState>()
            .init_resource::<StepCountRun>()
            .init_resource::<StepCountWindow>()
            .init_resource::<StopReports>();
        app.add_systems(OnEnter(PlayModeState::Playing), (unpause_physics, open_step_count_run));
        app.add_systems(OnEnter(PlayModeState::Editing), stop_run);
        app.add_systems(
            PreUpdate,
            (sample_step_window, advance_simulation_clock)
                .chain()
                .run_if(in_state(PlayModeState::Playing)),
        );
        // Warm-up in Edit: the first frames settle the clocks.
        app.update();
        app.update();
        hold_physics(&mut app.world_mut().resource_mut::<Time<Physics>>());
        app.world_mut().resource_mut::<StopReports>().0.clear();
        app
    }

    fn set_state(app: &mut App, state: PlayModeState) {
        app.world_mut().resource_mut::<NextState<PlayModeState>>().set(state);
    }

    /// Enter Play, run `frames` frames, stop; return the stop's report.
    fn play_for(app: &mut App, frames: u32) -> StepCountReport {
        let counter_before = executed(app);
        set_state(app, PlayModeState::Playing);
        app.update();
        for _ in 0..frames {
            app.update();
        }
        set_state(app, PlayModeState::Editing);
        app.update();
        let report = app.world_mut().resource_mut::<StopReports>().0.pop().expect("the stop wrote a report");
        assert_eq!(
            report.physics_steps_before_clock_start + report.physics_steps_executed,
            executed(app) - counter_before,
            "every step of the run is either before the window or in it",
        );
        report
    }

    /// One fixed step per frame at scale 1: the frame that enters Play steps
    /// physics before the clock first advances, and the frame that leaves
    /// Play advances the clock after physics last stepped. Both are reported
    /// on their own and neither is in the window, so the window drops
    /// exactly nothing. Opening the window at `OnEnter(Playing)` again
    /// counts the first and makes `dropped_steps` -1; closing it after the
    /// last advance counts the second and makes it +1.
    #[test]
    fn window_opens_at_the_first_clock_advance_and_closes_before_the_last() {
        let step = Time::<Fixed>::from_hz(60.0).timestep();
        let mut app = play_app(step);
        let report = play_for(&mut app, 600);
        assert_eq!(report.physics_steps_before_clock_start, 1);
        assert_eq!(report.physics_steps_executed, 600);
        assert_eq!(report.clock_ticks_after_physics_end, 1);
        assert_eq!(report.clock_tick_count_implied_by_time, 600);
        assert_eq!(report.dropped_steps, 0);
        assert_eq!(report.fixed_overstep_at_start_s, report.fixed_overstep_at_end_s);
    }

    /// A pause in the middle of a run keeps the window's start, and the pair
    /// of edge frames it adds (the clock advance of the frame that pauses,
    /// the physics step of the frame that resumes) cancel at one step per
    /// frame.
    #[test]
    fn a_resume_keeps_the_runs_window() {
        let step = Time::<Fixed>::from_hz(60.0).timestep();
        let mut app = play_app(step);
        let counter_before = executed(&app);
        set_state(&mut app, PlayModeState::Playing);
        app.update();
        for _ in 0..100 {
            app.update();
        }
        let start = app.world().resource::<StepCountWindow>().start.expect("the window opened");
        set_state(&mut app, PlayModeState::Paused);
        hold_physics(&mut app.world_mut().resource_mut::<Time<Physics>>());
        for _ in 0..10 {
            app.update();
        }
        set_state(&mut app, PlayModeState::Playing);
        for _ in 0..100 {
            app.update();
        }
        assert_eq!(app.world().resource::<StepCountWindow>().start, Some(start), "a resume keeps the start");
        set_state(&mut app, PlayModeState::Editing);
        app.update();
        let report = app.world_mut().resource_mut::<StopReports>().0.pop().expect("the stop wrote a report");
        assert_eq!(report.physics_step_counter_at_start, start.physics_step_counter);
        assert_eq!(report.physics_steps_before_clock_start, 1);
        assert_eq!(report.physics_step_counter_at_start, counter_before + 1);
        assert_eq!(report.dropped_steps, 0);
    }

    /// Frames of 4/3 of a step: the fixed loop steps one or two times a
    /// frame and carries a remainder, so the window's edges fall mid-step.
    /// The exported oversteps account for `dropped_steps` exactly: it is
    /// the rounded difference between them, and zero only when they are
    /// within half a step of each other.
    #[test]
    fn oversteps_account_for_the_phase_at_the_windows_edges() {
        let step = Time::<Fixed>::from_hz(60.0).timestep();
        let mut app = play_app(step * 4 / 3);
        for frames in 1..=9 {
            let report = play_for(&mut app, frames);
            let phase = (report.fixed_overstep_at_end_s - report.fixed_overstep_at_start_s) / report.fixed_timestep_s;
            assert_eq!(report.dropped_steps, phase.round() as i64, "{frames} frames: {report:?}");
            for _ in 0..3 {
                app.update();
            }
        }
    }
}
