//! Real-time Eustress Streams listener for Workshop AI awareness.
//!
//! Subscribes to key stream topics and maintains a compressed summary
//! of recent events. This summary is injected into the Claude system
//! prompt so the AI has live awareness of what's happening in the
//! simulation.

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Maximum number of recent events to keep in memory.
const MAX_RECENT_EVENTS: usize = 50;

/// A single stream event captured for AI awareness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamEvent {
    /// Stream topic this event came from.
    pub topic: String,
    /// Event summary (human-readable, one line).
    pub summary: String,
    /// Timestamp (ISO 8601).
    pub timestamp: String,
}

/// Maintains real-time awareness of the running simulation
/// via Eustress Streams subscriptions.
#[derive(Resource)]
pub struct StreamAwareContext {
    /// Recent stream events (bounded ring buffer).
    pub recent_events: VecDeque<StreamEvent>,
    /// Compressed world model summary (regenerated periodically).
    pub world_summary: String,
    /// Entity count from last snapshot.
    pub entity_count: u32,
    /// Active simulation state.
    pub simulation_running: bool,
    /// Current simulation time (seconds).
    pub simulation_time: f64,
}

impl Default for StreamAwareContext {
    fn default() -> Self {
        Self {
            recent_events: VecDeque::with_capacity(MAX_RECENT_EVENTS),
            world_summary: String::new(),
            entity_count: 0,
            simulation_running: false,
            simulation_time: 0.0,
        }
    }
}

impl StreamAwareContext {
    /// Push a new event, evicting the oldest if at capacity.
    pub fn push_event(&mut self, topic: &str, summary: &str) {
        if self.recent_events.len() >= MAX_RECENT_EVENTS {
            self.recent_events.pop_front();
        }
        self.recent_events.push_back(StreamEvent {
            topic: topic.to_string(),
            summary: summary.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
    }

    /// Update the world summary from a snapshot.
    pub fn update_world_summary(&mut self, summary: String, entity_count: u32) {
        self.world_summary = summary;
        self.entity_count = entity_count;
    }

    /// Format the stream context for injection into the Claude system prompt.
    /// Includes world summary + last N events.
    pub fn format_for_prompt(&self) -> String {
        let mut out = String::new();

        // World state
        out.push_str(&format!(
            "## Live World State\nEntities: {} | Simulation: {} | Time: {:.1}s\n",
            self.entity_count,
            if self.simulation_running { "Running" } else { "Stopped" },
            self.simulation_time,
        ));

        if !self.world_summary.is_empty() {
            out.push_str(&format!("{}\n", self.world_summary));
        }

        // Recent events
        if !self.recent_events.is_empty() {
            out.push_str("\n## Recent Events\n");
            // Show last 10 for prompt brevity
            let start = self.recent_events.len().saturating_sub(10);
            for event in self.recent_events.iter().skip(start) {
                out.push_str(&format!("- [{}] {}\n", event.topic, event.summary));
            }
        }

        out
    }
}
