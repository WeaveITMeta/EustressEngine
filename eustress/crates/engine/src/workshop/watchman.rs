//! # Watchman — Proactive Simulation Monitor
//!
//! Monitors live telemetry during simulation playback and injects
//! agent-directed messages into the Workshop pipeline when anomalies
//! are detected. This is the engine-side half of the proactive
//! feedback loop documented in `docs/architecture/SITL_HIL_ARCHITECTURE.md`.
//!
//! ## Table of Contents
//!
//! 1. WatchmanConfig — threshold configuration resource
//! 2. WatchmanState — runtime tracking (cooldown, last-alert state)
//! 3. watchman_monitor — Bevy system that reads SimValuesResource and
//!    injects Repairman-addressed messages into IdeationPipeline
//!
//! ## Feedback Loop
//!
//! ```text
//! SimValuesResource (every frame)
//!        │
//!        ▼
//! watchman_monitor (5-second poll)
//!        │ detects: value outside threshold
//!        ▼
//! IdeationPipeline.add_user_message(...)
//!        │ synthetic "[Watchman] ..." message
//!        ▼
//! dispatch_chat_request (next frame)
//!        │ Claude sees the alert + has sim tools
//!        ▼
//! Claude responds with diagnosis + tool calls
//!        (set_sim_value, feedback_diff, etc.)
//! ```
//!
//! ## Design Decisions
//!
//! - The Watchman does NOT call Claude directly. It injects a message
//!   into the existing pipeline so the full tool-use loop runs normally
//!   with approval gates, audit logging, and UI visibility.
//! - A 30-second cooldown prevents alert storms when a value oscillates
//!   around a threshold boundary.
//! - Alerts are only emitted during PlayModeState::Playing.
//! - The system is opt-in: `WatchmanConfig.enabled` defaults to `true`
//!   but can be toggled from the Workshop UI or via MCP.

use bevy::prelude::*;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::play_mode::PlayModeState;
use crate::simulation::plugin::SimValuesResource;

// ============================================================================
// Configuration
// ============================================================================

/// Threshold rule for a single watchpoint key.
#[derive(Debug, Clone)]
pub struct WatchmanThreshold {
    /// Watchpoint key (e.g. "battery.temperature_c")
    pub key: String,
    /// Maximum allowed value. Alert if exceeded.
    pub max: Option<f64>,
    /// Minimum allowed value. Alert if below.
    pub min: Option<f64>,
    /// Human-readable label for alert messages.
    pub label: String,
    /// Unit string for formatting (e.g. "°C", "V", "%").
    pub unit: String,
}

/// Top-level Watchman configuration. Inserted as a Bevy Resource.
#[derive(Resource)]
pub struct WatchmanConfig {
    /// Master toggle — when false, no alerts fire.
    pub enabled: bool,
    /// Threshold rules. Keyed by watchpoint name for O(1) lookup.
    pub thresholds: HashMap<String, WatchmanThreshold>,
    /// How often the monitor polls (default: 5 seconds).
    pub poll_interval: Duration,
    /// Cooldown after an alert fires before the same key can alert again.
    pub alert_cooldown: Duration,
    /// Maximum number of proactive alerts per simulation run to prevent
    /// runaway Claude calls.
    pub max_alerts_per_run: u32,
}

impl Default for WatchmanConfig {
    fn default() -> Self {
        let mut thresholds = HashMap::new();

        // Default battery safety thresholds (VCell case study)
        let defaults = [
            ("battery.temperature_c", None, Some(60.0), "Cell Temperature", "°C"),
            ("battery.voltage", Some(2.5), Some(4.25), "Cell Voltage", "V"),
            ("battery.soc", Some(0.0), Some(100.0), "State of Charge", "%"),
            ("battery.dendrite_risk", None, Some(80.0), "Dendrite Risk", "%"),
            ("battery.capacity_retention", Some(70.0), None, "Capacity Retention", "%"),
        ];

        for (key, min, max, label, unit) in defaults {
            thresholds.insert(key.to_string(), WatchmanThreshold {
                key: key.to_string(),
                max,
                min,
                label: label.to_string(),
                unit: unit.to_string(),
            });
        }

        Self {
            enabled: true,
            thresholds,
            poll_interval: Duration::from_secs(5),
            alert_cooldown: Duration::from_secs(30),
            max_alerts_per_run: 10,
        }
    }
}

// ============================================================================
// Runtime State
// ============================================================================

/// Tracks per-key cooldowns and alert counts.
#[derive(Resource, Default)]
pub struct WatchmanState {
    /// Last time the monitor polled.
    last_poll: Option<Instant>,
    /// Per-key: when was the last alert fired?
    last_alert: HashMap<String, Instant>,
    /// Total alerts fired in the current simulation run.
    alerts_fired: u32,
}

impl WatchmanState {
    /// Reset counters when a new simulation run starts.
    pub fn reset(&mut self) {
        self.last_alert.clear();
        self.alerts_fired = 0;
        self.last_poll = None;
    }
}

// ============================================================================
// Bevy System — the actual monitor
// ============================================================================

/// Polls SimValuesResource at `WatchmanConfig.poll_interval` and injects
/// alert messages into the Workshop IdeationPipeline when thresholds are
/// breached. Runs only during PlayModeState::Playing.
pub fn watchman_monitor(
    config: Res<WatchmanConfig>,
    mut state: ResMut<WatchmanState>,
    sim_values: Res<SimValuesResource>,
    mut pipeline: ResMut<super::IdeationPipeline>,
) {
    if !config.enabled { return }
    if state.alerts_fired >= config.max_alerts_per_run { return }

    // Throttle polling
    let now = Instant::now();
    if let Some(last) = state.last_poll {
        if now.duration_since(last) < config.poll_interval { return }
    }
    state.last_poll = Some(now);

    // Check each threshold against current sim values
    let mut alerts: Vec<String> = Vec::new();

    for (key, threshold) in &config.thresholds {
        let Some(value) = sim_values.0.get(key) else { continue };

        let mut breached = false;
        let mut direction = String::new();

        if let Some(max) = threshold.max {
            if *value > max {
                breached = true;
                direction = format!("exceeded max ({:.2}{} > {:.2}{})",
                    value, threshold.unit, max, threshold.unit);
            }
        }
        if let Some(min) = threshold.min {
            if *value < min {
                breached = true;
                direction = format!("below min ({:.2}{} < {:.2}{})",
                    value, threshold.unit, min, threshold.unit);
            }
        }

        if !breached { continue }

        // Check cooldown for this key
        if let Some(last_alert_time) = state.last_alert.get(key) {
            if now.duration_since(*last_alert_time) < config.alert_cooldown {
                continue;
            }
        }

        alerts.push(format!("⚠️ {} — {} {}", threshold.label, key, direction));
        state.last_alert.insert(key.clone(), now);
    }

    if alerts.is_empty() { return }

    // Inject a synthetic Watchman alert into the Workshop pipeline.
    // This message looks like a user message so the dispatch guard
    // in dispatch_chat_request sees it and fires a Claude turn.
    let alert_body = format!(
        "[Watchman Alert] The following simulation thresholds were breached:\n\n{}\n\n\
        As the Repairman agent, diagnose the root cause. Use `tail_telemetry` to see recent trends, \
        `get_simulation_state` for current values, and `feedback_diff` to compare against the last \
        known-good state. Propose a targeted fix using `set_sim_value` or a script edit, then \
        re-run the simulation to verify.",
        alerts.join("\n"),
    );

    pipeline.add_user_message(alert_body);
    state.alerts_fired += alerts.len() as u32;

    info!("Watchman: {} alert(s) injected into Workshop pipeline (total: {})",
        alerts.len(), state.alerts_fired);
}

/// Reset Watchman state when entering Play mode (fresh run).
pub fn watchman_reset_on_play(mut state: ResMut<WatchmanState>) {
    state.reset();
    info!("Watchman: state reset for new simulation run");
}
