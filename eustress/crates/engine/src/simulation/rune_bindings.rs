//! # Simulation Rune Bindings
//!
//! Exposes simulation control and data access to Rune scripts.

use bevy::prelude::*;
use eustress_common::simulation::{
    SimulationClock, SimulationState, SimulationMode,
    WatchPointRegistry, BreakPointRegistry, WatchPoint, BreakPoint, Comparison,
    presets,
};
use std::sync::{Arc, RwLock};

/// Rune bindings for simulation control
/// 
/// This struct provides thread-safe access to simulation resources
/// for use in Rune scripts.
#[derive(Clone)]
pub struct SimulationRuneBindings {
    clock: Arc<RwLock<SimulationClockSnapshot>>,
    state: Arc<RwLock<SimulationStateSnapshot>>,
    watchpoints: Arc<RwLock<WatchPointSnapshot>>,
}

/// Snapshot of clock state for Rune access
#[derive(Clone, Debug, Default)]
pub struct SimulationClockSnapshot {
    pub simulation_time_s: f64,
    pub wall_time_s: f64,
    pub time_scale: f64,
    pub tick_count: u64,
    pub dt: f64,
}

/// Snapshot of simulation state for Rune access
#[derive(Clone, Debug, Default)]
pub struct SimulationStateSnapshot {
    pub is_running: bool,
    pub is_paused: bool,
    pub is_completed: bool,
    pub completion_reason: Option<String>,
}

/// Snapshot of watchpoint values for Rune access
#[derive(Clone, Debug, Default)]
pub struct WatchPointSnapshot {
    pub values: std::collections::HashMap<String, f64>,
    pub mins: std::collections::HashMap<String, f64>,
    pub maxs: std::collections::HashMap<String, f64>,
    pub averages: std::collections::HashMap<String, f64>,
}

impl Default for SimulationRuneBindings {
    fn default() -> Self {
        Self {
            clock: Arc::new(RwLock::new(SimulationClockSnapshot::default())),
            state: Arc::new(RwLock::new(SimulationStateSnapshot::default())),
            watchpoints: Arc::new(RwLock::new(WatchPointSnapshot::default())),
        }
    }
}

impl SimulationRuneBindings {
    /// Create new bindings
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Update snapshots from ECS resources (call from Bevy system)
    pub fn sync_from_ecs(
        &self,
        clock: &SimulationClock,
        state: &SimulationState,
        watchpoints: &WatchPointRegistry,
    ) {
        if let Ok(mut snap) = self.clock.write() {
            snap.simulation_time_s = clock.simulation_time_s;
            snap.wall_time_s = clock.wall_time_s;
            snap.time_scale = clock.time_scale;
            snap.tick_count = clock.tick_count;
            snap.dt = clock.fixed_timestep_s;
        }
        
        if let Ok(mut snap) = self.state.write() {
            snap.is_running = matches!(state.mode, SimulationMode::Running);
            snap.is_paused = matches!(state.mode, SimulationMode::Paused);
            snap.is_completed = state.completed;
            snap.completion_reason = state.completion_reason.clone();
        }
        
        if let Ok(mut snap) = self.watchpoints.write() {
            snap.values.clear();
            snap.mins.clear();
            snap.maxs.clear();
            snap.averages.clear();
            
            for (name, wp) in &watchpoints.watchpoints {
                snap.values.insert(name.clone(), wp.current);
                snap.mins.insert(name.clone(), wp.min);
                snap.maxs.insert(name.clone(), wp.max);
                snap.averages.insert(name.clone(), wp.average);
            }
        }
    }
    
    /// Get current simulation time in seconds
    pub fn time(&self) -> f64 {
        self.clock.read().map(|c| c.simulation_time_s).unwrap_or(0.0)
    }
    
    /// Get simulation time in hours
    pub fn time_hours(&self) -> f64 {
        self.time() / 3600.0
    }
    
    /// Get simulation time in days
    pub fn time_days(&self) -> f64 {
        self.time() / 86400.0
    }
    
    /// Get simulation time in years
    pub fn time_years(&self) -> f64 {
        self.time() / 31536000.0
    }
    
    /// Get wall clock time elapsed
    pub fn wall_time(&self) -> f64 {
        self.clock.read().map(|c| c.wall_time_s).unwrap_or(0.0)
    }
    
    /// Get current time scale
    pub fn time_scale(&self) -> f64 {
        self.clock.read().map(|c| c.time_scale).unwrap_or(1.0)
    }
    
    /// Get current tick count
    pub fn tick(&self) -> u64 {
        self.clock.read().map(|c| c.tick_count).unwrap_or(0)
    }
    
    /// Get timestep (dt) in simulation seconds
    pub fn dt(&self) -> f64 {
        self.clock.read().map(|c| c.dt).unwrap_or(1.0 / 60.0)
    }
    
    /// Check if simulation is running
    pub fn is_running(&self) -> bool {
        self.state.read().map(|s| s.is_running).unwrap_or(false)
    }
    
    /// Check if simulation is paused
    pub fn is_paused(&self) -> bool {
        self.state.read().map(|s| s.is_paused).unwrap_or(true)
    }
    
    /// Check if simulation has completed
    pub fn is_completed(&self) -> bool {
        self.state.read().map(|s| s.is_completed).unwrap_or(false)
    }
    
    /// Get completion reason if completed
    pub fn completion_reason(&self) -> Option<String> {
        self.state.read().ok().and_then(|s| s.completion_reason.clone())
    }
    
    /// Get current value of a watchpoint
    pub fn get(&self, name: &str) -> f64 {
        self.watchpoints.read()
            .ok()
            .and_then(|w| w.values.get(name).copied())
            .unwrap_or(0.0)
    }
    
    /// Get minimum value seen for a watchpoint
    pub fn get_min(&self, name: &str) -> f64 {
        self.watchpoints.read()
            .ok()
            .and_then(|w| w.mins.get(name).copied())
            .unwrap_or(f64::MAX)
    }
    
    /// Get maximum value seen for a watchpoint
    pub fn get_max(&self, name: &str) -> f64 {
        self.watchpoints.read()
            .ok()
            .and_then(|w| w.maxs.get(name).copied())
            .unwrap_or(f64::MIN)
    }
    
    /// Get average value for a watchpoint
    pub fn get_avg(&self, name: &str) -> f64 {
        self.watchpoints.read()
            .ok()
            .and_then(|w| w.averages.get(name).copied())
            .unwrap_or(0.0)
    }
    
    /// Get all watchpoint names
    pub fn watchpoint_names(&self) -> Vec<String> {
        self.watchpoints.read()
            .map(|w| w.values.keys().cloned().collect())
            .unwrap_or_default()
    }
    
    /// Get compression ratio (simulated time / wall time)
    pub fn compression_ratio(&self) -> f64 {
        let wall = self.wall_time();
        if wall > 0.0 {
            self.time() / wall
        } else {
            self.time_scale()
        }
    }
    
    /// Format simulation time as human-readable string
    pub fn format_time(&self) -> String {
        let t = self.time();
        if t < 60.0 {
            format!("{:.1}s", t)
        } else if t < 3600.0 {
            format!("{:.1}m", t / 60.0)
        } else if t < 86400.0 {
            format!("{:.1}h", t / 3600.0)
        } else if t < 31536000.0 {
            format!("{:.1}d", t / 86400.0)
        } else {
            format!("{:.2}y", t / 31536000.0)
        }
    }
}

/// Commands to send back to ECS from Rune scripts
#[derive(Clone, Debug)]
pub enum SimulationCommand {
    Pause,
    Resume,
    Step,
    StepN(u32),
    SetTimeScale(f64),
    RunUntilTime(f64),
    RunUntilTick(u64),
    Reset,
    RecordWatchpoint { name: String, value: f64 },
    AddWatchpoint { name: String, label: String, unit: String },
    AddBreakpoint { name: String, variable: String, comparison: String, threshold: f64 },
    RemoveBreakpoint { name: String },
    EnableBreakpoint { name: String, enabled: bool },
    StartRecording { name: String },
    StopRecording,
    ExportRecording { path: String },
}

/// Resource for queuing commands from Rune to ECS
#[derive(Resource, Default)]
pub struct SimulationCommandQueue {
    pub commands: Vec<SimulationCommand>,
}

impl SimulationCommandQueue {
    pub fn push(&mut self, cmd: SimulationCommand) {
        self.commands.push(cmd);
    }
    
    pub fn drain(&mut self) -> Vec<SimulationCommand> {
        std::mem::take(&mut self.commands)
    }
}

/// Rune-callable simulation controller
/// 
/// This is the main interface Rune scripts use to control simulation.
#[derive(Clone)]
pub struct SimController {
    bindings: SimulationRuneBindings,
    commands: Arc<RwLock<Vec<SimulationCommand>>>,
}

impl SimController {
    pub fn new(bindings: SimulationRuneBindings) -> Self {
        Self {
            bindings,
            commands: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    fn queue(&self, cmd: SimulationCommand) {
        if let Ok(mut cmds) = self.commands.write() {
            cmds.push(cmd);
        }
    }
    
    /// Pause simulation
    pub fn pause(&self) {
        self.queue(SimulationCommand::Pause);
    }
    
    /// Resume simulation
    pub fn resume(&self) {
        self.queue(SimulationCommand::Resume);
    }
    
    /// Step one tick
    pub fn step(&self) {
        self.queue(SimulationCommand::Step);
    }
    
    /// Step N ticks
    pub fn step_n(&self, n: u32) {
        self.queue(SimulationCommand::StepN(n));
    }
    
    /// Set time scale
    pub fn set_time_scale(&self, scale: f64) {
        self.queue(SimulationCommand::SetTimeScale(scale));
    }
    
    /// Set time scale to real-time
    pub fn realtime(&self) {
        self.set_time_scale(presets::REALTIME);
    }
    
    /// Set time scale for 1 hour per second
    pub fn fast_hour(&self) {
        self.set_time_scale(presets::FAST_1HOUR_PER_SEC);
    }
    
    /// Set time scale for 1 day per second
    pub fn fast_day(&self) {
        self.set_time_scale(presets::FAST_1DAY_PER_SEC);
    }
    
    /// Set time scale for 1 year per second
    pub fn fast_year(&self) {
        self.set_time_scale(presets::FAST_1YEAR_PER_SEC);
    }
    
    /// Set time scale for battery cycle testing
    pub fn battery_test(&self) {
        self.set_time_scale(presets::BATTERY_CYCLE_TEST);
    }
    
    /// Run until simulation time reaches target
    pub fn run_until_time(&self, target_s: f64) {
        self.queue(SimulationCommand::RunUntilTime(target_s));
    }
    
    /// Run until tick count reaches target
    pub fn run_until_tick(&self, target: u64) {
        self.queue(SimulationCommand::RunUntilTick(target));
    }
    
    /// Run for N simulated hours
    pub fn run_hours(&self, hours: f64) {
        let target = self.bindings.time() + hours * 3600.0;
        self.run_until_time(target);
    }
    
    /// Run for N simulated days
    pub fn run_days(&self, days: f64) {
        let target = self.bindings.time() + days * 86400.0;
        self.run_until_time(target);
    }
    
    /// Run for N simulated years
    pub fn run_years(&self, years: f64) {
        let target = self.bindings.time() + years * 31536000.0;
        self.run_until_time(target);
    }
    
    /// Reset simulation to initial state
    pub fn reset(&self) {
        self.queue(SimulationCommand::Reset);
    }
    
    /// Record a value to a watchpoint
    pub fn record(&self, name: &str, value: f64) {
        self.queue(SimulationCommand::RecordWatchpoint {
            name: name.to_string(),
            value,
        });
    }
    
    /// Add a new watchpoint
    pub fn add_watchpoint(&self, name: &str, label: &str, unit: &str) {
        self.queue(SimulationCommand::AddWatchpoint {
            name: name.to_string(),
            label: label.to_string(),
            unit: unit.to_string(),
        });
    }
    
    /// Add a breakpoint
    pub fn add_breakpoint(&self, name: &str, variable: &str, comparison: &str, threshold: f64) {
        self.queue(SimulationCommand::AddBreakpoint {
            name: name.to_string(),
            variable: variable.to_string(),
            comparison: comparison.to_string(),
            threshold,
        });
    }
    
    /// Remove a breakpoint
    pub fn remove_breakpoint(&self, name: &str) {
        self.queue(SimulationCommand::RemoveBreakpoint {
            name: name.to_string(),
        });
    }
    
    /// Enable or disable a breakpoint
    pub fn enable_breakpoint(&self, name: &str, enabled: bool) {
        self.queue(SimulationCommand::EnableBreakpoint {
            name: name.to_string(),
            enabled,
        });
    }
    
    /// Start recording
    pub fn start_recording(&self, name: &str) {
        self.queue(SimulationCommand::StartRecording {
            name: name.to_string(),
        });
    }
    
    /// Stop recording
    pub fn stop_recording(&self) {
        self.queue(SimulationCommand::StopRecording);
    }
    
    /// Export recording to file
    pub fn export(&self, path: &str) {
        self.queue(SimulationCommand::ExportRecording {
            path: path.to_string(),
        });
    }
    
    /// Get current simulation time
    pub fn time(&self) -> f64 {
        self.bindings.time()
    }
    
    /// Get current tick
    pub fn tick(&self) -> u64 {
        self.bindings.tick()
    }
    
    /// Get watchpoint value
    pub fn get(&self, name: &str) -> f64 {
        self.bindings.get(name)
    }
    
    /// Check if running
    pub fn is_running(&self) -> bool {
        self.bindings.is_running()
    }
    
    /// Check if completed
    pub fn is_completed(&self) -> bool {
        self.bindings.is_completed()
    }
    
    /// Drain pending commands (call from ECS system)
    pub fn drain_commands(&self) -> Vec<SimulationCommand> {
        self.commands.write()
            .map(|mut c| std::mem::take(&mut *c))
            .unwrap_or_default()
    }
}
