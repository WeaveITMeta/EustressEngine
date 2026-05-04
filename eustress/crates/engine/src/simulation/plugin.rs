//! # Simulation Plugin
//!
//! Core Bevy plugin for tick-based simulation with time compression.
//! Integrates with PlayModeState for proper play/pause/stop behavior.

use bevy::prelude::*;
use tracing::warn;
use eustress_common::simulation::{
    SimulationClock, SimulationState, SimulationMode,
    WatchPointRegistry, BreakPointRegistry,
    SimulationRecording, TimeSeries, WatchPoint, BreakPoint, Comparison,
};

use crate::play_mode::PlayModeState;

/// Bevy Resource mirror of SIM_VALUES thread-local.
/// Written by `publish_echem_to_sim_values` (Update), read by `record_and_stream_watchpoints` (PostUpdate).
/// Avoids thread-local cross-thread visibility issues in Bevy's multi-threaded executor.
#[derive(Resource, Default)]
pub struct SimValuesResource(pub std::collections::HashMap<String, f64>);

/// Core simulation plugin providing tick-based time compression
#[derive(Default)]
pub struct SimulationPlugin;

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SimulationClock>()
            .init_resource::<SimulationState>()
            .init_resource::<SimValuesResource>()
            .init_resource::<WatchPointRegistry>()
            .init_resource::<BreakPointRegistry>()
            .init_resource::<ActiveRecording>()
            .init_resource::<TelemetryWriterState>()
            .register_type::<SimulationClock>()
            .register_type::<SimulationState>()
            // Sync simulation state with play mode transitions
            .add_systems(OnEnter(PlayModeState::Playing), on_play_start)
            .add_systems(OnEnter(PlayModeState::Playing), register_battery_watchpoints)
            .add_systems(OnEnter(PlayModeState::Paused), on_play_pause)
            .add_systems(OnEnter(PlayModeState::Editing), on_play_stop)
            // Drain MCP sim-commands.jsonl every frame (any state)
            .add_systems(PreUpdate, drain_sim_commands)
            // Advance simulation clock when playing
            .add_systems(
                PreUpdate,
                advance_simulation_clock
                    .run_if(in_state(PlayModeState::Playing))
                    .after(drain_sim_commands),
            )
            // Record watchpoint values + publish to stream each frame
            .add_systems(
                PostUpdate,
                record_and_stream_watchpoints.run_if(in_state(PlayModeState::Playing)),
            )
            // Write telemetry.jsonl for tail_telemetry MCP tool (1 Hz)
            .add_systems(
                PostUpdate,
                write_telemetry_log
                    .run_if(in_state(PlayModeState::Playing))
                    .after(record_and_stream_watchpoints),
            );
    }
}

/// Called when entering Playing state - ensure simulation is running
fn on_play_start(mut sim_state: ResMut<SimulationState>) {
    // Always ensure Running mode when entering play
    sim_state.mode = SimulationMode::Running;
    sim_state.completed = false;
    info!("🎮 Simulation started (mode=Running)");
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
    mut watchpoints: ResMut<WatchPointRegistry>,
    mut breakpoints: ResMut<BreakPointRegistry>,
    mut recording: ResMut<ActiveRecording>,
    mut output: Option<ResMut<crate::ui::slint_ui::OutputConsole>>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
) {
    // Stop and auto-export recording BEFORE resetting clock (clock.reset() zeros tick_count)
    if recording.enabled {
        // Write final clock state into recording metadata before stopping
        if let Some(ref mut rec) = recording.recording {
            rec.metadata.total_ticks = sim_clock.tick_count;
            rec.metadata.simulation_duration_s = sim_clock.simulation_time_s;
            rec.metadata.wall_duration_s = sim_clock.wall_time_s;
        }
        if let Some(rec) = recording.stop() {
            let ticks = rec.metadata.total_ticks;
            let sim_duration = rec.metadata.simulation_duration_s;
            let series_count = rec.series.len();
            info!("📊 Simulation recording stopped: {} ticks, {:.2}s simulated, {} watchpoints",
                ticks, sim_duration, series_count);

            // Auto-export to Universe knowledge/recordings/{space_name}/
            let recordings_dir = if let Some(ref sr) = space_root {
                let space_name = sr.0.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("default");
                // Walk up from space root: spaces/SpaceName → Universe root
                let universe_root = sr.0.parent() // spaces/
                    .and_then(|p| p.parent()); // Universe root
                if let Some(ur) = universe_root {
                    ur.join(".eustress").join("knowledge").join("recordings").join(space_name)
                } else {
                    sr.0.join(".eustress").join("recordings")
                }
            } else {
                // Fallback if no space root
                crate::space::workspace_root().join(".eustress").join("recordings")
            };
            {
                if let Err(e) = std::fs::create_dir_all(&recordings_dir) {
                    warn!("Failed to create recordings dir: {}", e);
                } else {
                    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
                    let json_path = recordings_dir.join(format!("sim_{}.json", timestamp));
                    match rec.export_json(&json_path) {
                        Ok(_) => {
                            let msg = format!("Recording exported to {}", json_path.display());
                            info!("💾 {}", msg);
                            if let Some(ref mut out) = output {
                                out.info(msg);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to export recording: {}", e);
                            if let Some(ref mut out) = output {
                                out.error(format!("Failed to export recording: {}", e));
                            }
                        }
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

    // Reset AFTER recording is saved — so tick_count and sim_time are preserved in the export
    sim_clock.reset();
    sim_state.reset();
    watchpoints.reset_all();
    breakpoints.reset_all();

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

    // Start recording automatically
    recording.start("simulation_run");
    info!("📊 Registered {} battery watchpoints, recording started", battery_watchpoints.len());
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
// MCP Sim-Command Drain — reads sim-commands.jsonl written by MCP tools
// ============================================================================

/// Drain `<universe>/.eustress/sim-commands.jsonl` each frame.
///
/// MCP tools (`run_simulation`, `stop_simulation`, `set_sim_value`)
/// append JSON lines to this file. The engine reads and truncates it
/// every frame, translating commands into ECS state mutations.
///
/// This system runs in ANY play state so `run_simulation` can be
/// issued from Edit mode and `stop_simulation` from Playing mode.
fn drain_sim_commands(
    space_root: Option<Res<crate::space::SpaceRoot>>,
    mut sim_values_res: ResMut<SimValuesResource>,
    mut clock: ResMut<SimulationClock>,
    mut next_play_state: ResMut<NextState<PlayModeState>>,
) {
    let Some(sr) = space_root.as_deref() else { return };

    // Walk up to Universe root (parent of Spaces/)
    let universe = {
        let mut cur = sr.0.clone();
        let mut found = None;
        for _ in 0..16 {
            if cur.join("Spaces").is_dir() {
                found = Some(cur.clone());
                break;
            }
            if !cur.pop() { break; }
        }
        match found {
            Some(u) => u,
            None => return,
        }
    };

    let path = universe.join(".eustress").join("sim-commands.jsonl");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) if !c.is_empty() => c,
        _ => return,
    };

    // Truncate immediately so commands aren't re-processed on next frame
    let _ = std::fs::write(&path, "");

    for line in content.lines() {
        let Ok(cmd) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let op = cmd.get("op").and_then(|v| v.as_str()).unwrap_or("");

        match op {
            "set_sim_value" => {
                if let (Some(key), Some(value)) = (
                    cmd.get("key").and_then(|v| v.as_str()),
                    cmd.get("value").and_then(|v| v.as_f64()),
                ) {
                    sim_values_res.0.insert(key.to_string(), value);
                    // Also write to thread-local so Rune scripts see it
                    crate::soul::rune_ecs_module::SIM_VALUES.with(|sv| {
                        sv.borrow_mut().insert(key.to_string(), value);
                    });
                    info!("MCP: set_sim_value({} = {})", key, value);
                }
            }
            "run_simulation" => {
                if let Some(scale) = cmd.get("time_scale").and_then(|v| v.as_f64()) {
                    clock.set_time_scale(scale);
                }
                next_play_state.set(PlayModeState::Playing);
                info!("MCP: run_simulation (time_scale={:.1}x)", clock.time_scale);
            }
            "stop_simulation" => {
                next_play_state.set(PlayModeState::Editing);
                info!("MCP: stop_simulation");
            }
            _ => {
                warn!("MCP: unknown sim command op '{}'", op);
            }
        }
    }
}

// ============================================================================
// Telemetry Writer — appends to telemetry.jsonl for tail_telemetry MCP tool
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

/// Write one JSONL line per second to `<universe>/.eustress/telemetry.jsonl`.
///
/// Each line: `{ "t": "<rfc3339>", "values": { "key": f64, ... } }`
///
/// The file is append-only during a simulation run. It grows unbounded
/// (acceptable for alpha — a future rotation/compaction system will cap
/// it at ~10 MB). The `tail_telemetry` MCP tool reads the last N lines.
fn write_telemetry_log(
    mut state: ResMut<TelemetryWriterState>,
    sim_values_res: Res<SimValuesResource>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
) {
    if state.last_write.elapsed() < state.interval { return }

    let Some(sr) = space_root.as_deref() else { return };
    let sim_values = &sim_values_res.0;
    if sim_values.is_empty() { return }

    // Walk up to Universe root
    let universe = {
        let mut cur = sr.0.clone();
        let mut found = None;
        for _ in 0..16 {
            if cur.join("Spaces").is_dir() {
                found = Some(cur.clone());
                break;
            }
            if !cur.pop() { break; }
        }
        match found {
            Some(u) => u,
            None => return,
        }
    };

    let path = universe.join(".eustress").join("telemetry.jsonl");
    let entry = serde_json::json!({
        "t": chrono::Utc::now().to_rfc3339(),
        "values": sim_values,
    });

    let write_result = (|| -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .create(true).append(true).open(&path)?;
        writeln!(f, "{}", entry)?;
        Ok(())
    })();

    if let Err(e) = write_result {
        warn!("Failed to write telemetry log: {}", e);
    }
    state.last_write = std::time::Instant::now();
}
