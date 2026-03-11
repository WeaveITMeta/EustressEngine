//! # Simulation System
//!
//! Core simulation infrastructure for time-compressed physics simulation.
//! Enables running years of simulated time in seconds of wall time.
//!
//! ## Table of Contents
//!
//! 1. **SimulationClock** — Time tracking with compression
//! 2. **SimulationState** — Running/Paused/Stepping modes
//! 3. **WatchPoint** — Observable variable tracking
//! 4. **BreakPoint** — Conditional simulation pause
//! 5. **DataRecorder** — Time-series data collection
//! 6. **SimulationConfig** — TOML-loadable configuration

pub mod clock;
pub mod state;
pub mod watchpoint;
pub mod breakpoint;
pub mod recorder;
pub mod config;

pub use clock::*;
pub use state::*;
pub use watchpoint::*;
pub use breakpoint::*;
pub use recorder::*;
pub use config::*;
