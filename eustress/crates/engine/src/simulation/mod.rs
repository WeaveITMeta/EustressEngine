//! # Simulation Module
//!
//! Physics and electrochemistry simulation harnesses for rapid prototyping.
//!
//! ## Table of Contents
//!
//! 1. **SimulationPlugin** — Core tick-based simulation system
//! 2. **Commands** — run / pause / stop / set, the run ledger, the queue drain
//! 3. **IPC** — per-instance and per-Universe simulation file locations
//! 4. **Rune Bindings** — Script access to simulation state

pub mod plugin;
pub mod command;
pub mod ipc;
pub mod rune_bindings;
pub mod electrochemistry;
pub mod data_binding;

pub use plugin::SimulationPlugin;
pub use rune_bindings::SimulationRuneBindings;
pub use electrochemistry::ElectrochemistryPlugin;
