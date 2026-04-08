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

/// Core simulation plugin providing tick-based time compression
#[derive(Default)]
pub struct SimulationPlugin;

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SimulationClock>()
            .init_resource::<SimulationState>()
            .init_resource::<WatchPointRegistry>()
            .init_resource::<BreakPointRegistry>()
            .init_resource::<ActiveRecording>()
            .register_type::<SimulationClock>()
            .register_type::<SimulationState>()
            // Sync simulation state with play mode transitions
            .add_systems(OnEnter(PlayModeState::Playing), on_play_start)
            .add_systems(OnEnter(PlayModeState::Playing), register_battery_watchpoints)
            .add_systems(OnEnter(PlayModeState::Paused), on_play_pause)
            .add_systems(OnEnter(PlayModeState::Editing), on_play_stop)
            // Advance simulation clock when playing
            .add_systems(
                PreUpdate,
                advance_simulation_clock.run_if(in_state(PlayModeState::Playing)),
            )
            // Record watchpoint values + publish to stream each frame
            .add_systems(
                PostUpdate,
                record_and_stream_watchpoints.run_if(in_state(PlayModeState::Playing)),
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
    sim_clock.reset();
    sim_state.reset();
    watchpoints.reset_all();
    breakpoints.reset_all();
    
    // Stop and auto-export recording if active
    if recording.enabled {
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
    #[cfg(feature = "streaming")]
    change_queue: Option<Res<eustress_common::change_queue::ChangeQueue>>,
) {
    let sim_time = clock.simulation_time_s;
    let tick = clock.tick_count;

    // Read current sim values from the thread-local (populated by prepare_script_bindings)
    let sim_values: std::collections::HashMap<String, f64> =
        crate::soul::rune_ecs_module::SIM_VALUES.with(|sv| sv.borrow().clone());

    if sim_values.is_empty() {
        return;
    }

    // Record each value into its watchpoint
    for (key, value) in &sim_values {
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
    let triggered = breakpoints.check_all(&sim_values);
    for bp_name in &triggered {
        info!("🛑 Breakpoint '{}' triggered at tick {} (sim_time={:.2}s)", bp_name, tick, sim_time);
        sim_state.hit_breakpoint(bp_name);

        // Record breakpoint event in active recording
        if recording.enabled {
            if let Some(ref mut rec) = recording.recording {
                let mut data = std::collections::HashMap::new();
                // Snapshot all current values at breakpoint time
                for (k, v) in &sim_values {
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
