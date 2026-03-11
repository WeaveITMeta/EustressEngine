//! # Simulation State
//!
//! Running/Paused/Stepping modes for simulation control.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Current simulation execution mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Reflect, Serialize, Deserialize)]
pub enum SimulationMode {
    /// Simulation is running normally
    #[default]
    Running,
    
    /// Simulation is paused
    Paused,
    
    /// Execute single tick then pause
    StepOnce,
    
    /// Execute N ticks then pause
    StepN(u32),
    
    /// Run until breakpoint hit or target reached
    RunUntil,
}

/// Simulation state resource
#[derive(Resource, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct SimulationState {
    /// Current execution mode
    pub mode: SimulationMode,
    
    /// Target simulation time for RunUntil mode (seconds)
    pub run_until_time_s: Option<f64>,
    
    /// Target tick count for RunUntil mode
    pub run_until_tick: Option<u64>,
    
    /// Whether simulation has completed (reached end condition)
    pub completed: bool,
    
    /// Completion reason if completed
    pub completion_reason: Option<String>,
    
    /// Number of breakpoints currently hit
    pub breakpoints_hit: u32,
    
    /// Last breakpoint that caused pause
    pub last_breakpoint: Option<String>,
}

impl Default for SimulationState {
    fn default() -> Self {
        Self {
            mode: SimulationMode::Running,
            run_until_time_s: None,
            run_until_tick: None,
            completed: false,
            completion_reason: None,
            breakpoints_hit: 0,
            last_breakpoint: None,
        }
    }
}

impl SimulationState {
    /// Check if simulation should execute ticks
    pub fn should_tick(&self) -> bool {
        matches!(self.mode, 
            SimulationMode::Running | 
            SimulationMode::StepOnce | 
            SimulationMode::StepN(_) |
            SimulationMode::RunUntil
        ) && !self.completed
    }
    
    /// Pause the simulation
    pub fn pause(&mut self) {
        self.mode = SimulationMode::Paused;
    }
    
    /// Resume the simulation
    pub fn resume(&mut self) {
        self.mode = SimulationMode::Running;
        self.completed = false;
        self.completion_reason = None;
    }
    
    /// Step one tick
    pub fn step(&mut self) {
        self.mode = SimulationMode::StepOnce;
    }
    
    /// Step N ticks
    pub fn step_n(&mut self, n: u32) {
        self.mode = SimulationMode::StepN(n);
    }
    
    /// Run until simulation time reaches target
    pub fn run_until_time(&mut self, target_s: f64) {
        self.mode = SimulationMode::RunUntil;
        self.run_until_time_s = Some(target_s);
        self.run_until_tick = None;
    }
    
    /// Run until tick count reaches target
    pub fn run_until_ticks(&mut self, target_tick: u64) {
        self.mode = SimulationMode::RunUntil;
        self.run_until_tick = Some(target_tick);
        self.run_until_time_s = None;
    }
    
    /// Mark simulation as completed
    pub fn complete(&mut self, reason: &str) {
        self.completed = true;
        self.completion_reason = Some(reason.to_string());
        self.mode = SimulationMode::Paused;
    }
    
    /// Record breakpoint hit
    pub fn hit_breakpoint(&mut self, name: &str) {
        self.breakpoints_hit += 1;
        self.last_breakpoint = Some(name.to_string());
        self.mode = SimulationMode::Paused;
    }
    
    /// Process tick completion, returns true if should continue
    pub fn after_tick(&mut self, current_time_s: f64, current_tick: u64) -> bool {
        match self.mode {
            SimulationMode::StepOnce => {
                self.mode = SimulationMode::Paused;
                false
            }
            SimulationMode::StepN(n) => {
                if n <= 1 {
                    self.mode = SimulationMode::Paused;
                    false
                } else {
                    self.mode = SimulationMode::StepN(n - 1);
                    true
                }
            }
            SimulationMode::RunUntil => {
                let time_reached = self.run_until_time_s
                    .map(|t| current_time_s >= t)
                    .unwrap_or(false);
                let tick_reached = self.run_until_tick
                    .map(|t| current_tick >= t)
                    .unwrap_or(false);
                
                if time_reached || tick_reached {
                    self.complete("Target reached");
                    false
                } else {
                    true
                }
            }
            SimulationMode::Running => true,
            SimulationMode::Paused => false,
        }
    }
    
    /// Reset to initial state
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
