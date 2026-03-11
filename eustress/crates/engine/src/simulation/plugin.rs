//! # Simulation Plugin
//!
//! Core Bevy plugin for tick-based simulation with time compression.
//! Integrates with PlayModeState for proper play/pause/stop behavior.

use bevy::prelude::*;
use eustress_common::simulation::{
    SimulationClock, SimulationState, SimulationMode,
    WatchPointRegistry, BreakPointRegistry,
    SimulationRecording, TimeSeries, WatchPoint, BreakPoint, Comparison,
};
use std::collections::HashMap;

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
            .add_systems(OnEnter(PlayModeState::Paused), on_play_pause)
            .add_systems(OnEnter(PlayModeState::Editing), on_play_stop)
            // Advance simulation clock when playing
            .add_systems(
                PreUpdate,
                advance_simulation_clock.run_if(in_state(PlayModeState::Playing)),
            );
    }
}

/// Called when entering Playing state - resume simulation
fn on_play_start(mut sim_state: ResMut<SimulationState>) {
    if sim_state.mode == SimulationMode::Paused {
        sim_state.resume();
    }
    info!("🎮 Simulation started/resumed");
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
) {
    sim_clock.reset();
    sim_state.reset();
    watchpoints.reset_all();
    breakpoints.reset_all();
    
    // Stop and export recording if active
    if recording.enabled {
        if let Some(rec) = recording.stop() {
            info!("📊 Simulation recording stopped: {} ticks, {:.2}s simulated", 
                rec.metadata.total_ticks, rec.metadata.simulation_duration_s);
        }
    }
    
    info!("⏹️ Simulation stopped and reset");
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
