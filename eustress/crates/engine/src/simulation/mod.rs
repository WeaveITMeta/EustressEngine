//! # Simulation Module
//!
//! Physics and electrochemistry simulation harnesses for rapid prototyping.
//!
//! ## Table of Contents
//!
//! 1. **SimulationPlugin** — Core tick-based simulation system
//! 2. **Rune Bindings** — Script access to simulation state

pub mod plugin;
pub mod rune_bindings;

pub use plugin::SimulationPlugin;
pub use rune_bindings::SimulationRuneBindings;
