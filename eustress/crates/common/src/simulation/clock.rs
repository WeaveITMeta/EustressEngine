//! # Simulation Clock
//!
//! Time tracking with compression for accelerated simulation.
//! Enables compressing years of simulated time into seconds of wall time.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Simulation clock resource tracking simulation time vs wall time.
///
/// Supports time compression factors from 1x (real-time) to 10^9x (years → seconds).
#[derive(Resource, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct SimulationClock {
    /// Current simulation time in seconds since simulation start
    pub simulation_time_s: f64,
    
    /// Total wall time elapsed in seconds
    pub wall_time_s: f64,
    
    /// Time compression factor (1.0 = real-time, 1e6 = 1 million x faster)
    pub time_scale: f64,
    
    /// Fixed timestep for physics in simulation seconds
    pub fixed_timestep_s: f64,
    
    /// Accumulated time for fixed timestep processing
    pub accumulator_s: f64,
    
    /// Total ticks executed since simulation start
    pub tick_count: u64,
    
    /// Target ticks per wall-clock second (default 60)
    pub tick_rate_hz: f64,
    
    /// Maximum ticks per frame to prevent spiral of death
    pub max_ticks_per_frame: u32,
}

impl Default for SimulationClock {
    fn default() -> Self {
        Self {
            simulation_time_s: 0.0,
            wall_time_s: 0.0,
            time_scale: 1.0,
            fixed_timestep_s: 1.0 / 60.0,
            accumulator_s: 0.0,
            tick_count: 0,
            tick_rate_hz: 60.0,
            max_ticks_per_frame: 10,
        }
    }
}

impl SimulationClock {
    /// Create a new simulation clock with given time scale
    pub fn new(time_scale: f64) -> Self {
        Self {
            time_scale: time_scale.max(0.0),
            ..default()
        }
    }
    
    /// Create clock for accelerated simulation (e.g., battery cycling)
    pub fn accelerated(time_scale: f64, tick_rate_hz: f64) -> Self {
        Self {
            time_scale: time_scale.max(0.0),
            tick_rate_hz: tick_rate_hz.max(1.0),
            fixed_timestep_s: 1.0 / tick_rate_hz.max(1.0),
            ..default()
        }
    }
    
    /// Advance clock by wall time delta, returns number of ticks to execute
    pub fn advance(&mut self, wall_delta_s: f64) -> u32 {
        self.wall_time_s += wall_delta_s;
        
        let sim_delta = wall_delta_s * self.time_scale;
        self.accumulator_s += sim_delta;
        
        let mut ticks = 0u32;
        while self.accumulator_s >= self.fixed_timestep_s && ticks < self.max_ticks_per_frame {
            self.accumulator_s -= self.fixed_timestep_s;
            self.simulation_time_s += self.fixed_timestep_s;
            self.tick_count += 1;
            ticks += 1;
        }
        
        if ticks >= self.max_ticks_per_frame {
            self.accumulator_s = 0.0;
        }
        
        ticks
    }
    
    /// Get simulation time in various units
    pub fn simulation_seconds(&self) -> f64 { self.simulation_time_s }
    pub fn simulation_minutes(&self) -> f64 { self.simulation_time_s / 60.0 }
    pub fn simulation_hours(&self) -> f64 { self.simulation_time_s / 3600.0 }
    pub fn simulation_days(&self) -> f64 { self.simulation_time_s / 86400.0 }
    pub fn simulation_years(&self) -> f64 { self.simulation_time_s / 31536000.0 }
    
    /// Get current timestep in simulation seconds
    pub fn dt(&self) -> f64 { self.fixed_timestep_s }
    
    /// Set time scale (clamped to non-negative)
    pub fn set_time_scale(&mut self, scale: f64) {
        self.time_scale = scale.max(0.0);
    }
    
    /// Reset simulation to initial state
    pub fn reset(&mut self) {
        self.simulation_time_s = 0.0;
        self.wall_time_s = 0.0;
        self.accumulator_s = 0.0;
        self.tick_count = 0;
    }
    
    /// Calculate compression ratio (simulated time / wall time)
    pub fn effective_compression(&self) -> f64 {
        if self.wall_time_s > 0.0 {
            self.simulation_time_s / self.wall_time_s
        } else {
            self.time_scale
        }
    }
}

/// Preset time scales for common simulation scenarios
pub mod presets {
    /// Real-time simulation (1 second = 1 second)
    pub const REALTIME: f64 = 1.0;
    
    /// Fast forward (1 minute = 1 second)
    pub const FAST_1MIN_PER_SEC: f64 = 60.0;
    
    /// Accelerated (1 hour = 1 second)
    pub const FAST_1HOUR_PER_SEC: f64 = 3600.0;
    
    /// Rapid (1 day = 1 second)
    pub const FAST_1DAY_PER_SEC: f64 = 86400.0;
    
    /// Ultra (1 week = 1 second)
    pub const FAST_1WEEK_PER_SEC: f64 = 604800.0;
    
    /// Extreme (1 month = 1 second)
    pub const FAST_1MONTH_PER_SEC: f64 = 2592000.0;
    
    /// Maximum (1 year = 1 second)
    pub const FAST_1YEAR_PER_SEC: f64 = 31536000.0;
    
    /// Battery cycling: 10,000 cycles (at 1C = 2h/cycle) in ~10 seconds
    pub const BATTERY_CYCLE_TEST: f64 = 7200000.0;
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn clock_advances_correctly() {
        let mut clock = SimulationClock::new(1.0);
        let ticks = clock.advance(1.0 / 60.0);
        assert_eq!(ticks, 1);
        assert_eq!(clock.tick_count, 1);
    }
    
    #[test]
    fn time_compression_works() {
        let mut clock = SimulationClock::new(3600.0);
        clock.advance(1.0);
        assert!((clock.simulation_hours() - 1.0).abs() < 0.1);
    }
    
    #[test]
    fn max_ticks_prevents_spiral() {
        let mut clock = SimulationClock::new(1e9);
        clock.max_ticks_per_frame = 5;
        let ticks = clock.advance(1.0);
        assert_eq!(ticks, 5);
    }
}
