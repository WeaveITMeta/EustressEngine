//! # Metrics Collection
//!
//! ## Table of Contents
//!
//! 1. **MetricsCollector** - Collects and reports server metrics
//! 2. **ServerMetrics** - Snapshot of server performance metrics

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Snapshot of server performance metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerMetrics {
    /// Server identifier
    pub server_id: String,
    /// CPU usage percentage (0-100)
    pub cpu_percent: f32,
    /// Memory usage in megabytes
    pub memory_mb: f32,
    /// Network bytes sent per second
    pub network_sent_bps: u64,
    /// Network bytes received per second
    pub network_recv_bps: u64,
    /// Current player count
    pub player_count: u32,
    /// Server tick rate (actual)
    pub tick_rate: f32,
    /// Average frame time in milliseconds
    pub frame_time_ms: f32,
    /// Entity count
    pub entity_count: u32,
    /// Timestamp of this snapshot
    pub timestamp: DateTime<Utc>,
}

/// Collects and reports server metrics to the Forge platform.
#[derive(Debug)]
pub struct MetricsCollector {
    /// Collection interval in seconds
    pub interval_secs: u64,
    /// History of collected metrics
    pub history: Vec<ServerMetrics>,
    /// Maximum history size
    pub max_history: usize,
    /// Whether collection is active
    pub active: bool,
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self {
            interval_secs: 10,
            history: Vec::new(),
            max_history: 360,
            active: false,
        }
    }
}

impl MetricsCollector {
    /// Create a new metrics collector with the given interval.
    pub fn new(interval_secs: u64) -> Self {
        Self {
            interval_secs,
            ..Default::default()
        }
    }

    /// Start collecting metrics.
    pub fn start(&mut self) {
        self.active = true;
    }

    /// Stop collecting metrics.
    pub fn stop(&mut self) {
        self.active = false;
    }

    /// Record a metrics snapshot.
    pub fn record(&mut self, metrics: ServerMetrics) {
        if self.history.len() >= self.max_history {
            self.history.remove(0);
        }
        self.history.push(metrics);
    }

    /// Get the latest metrics snapshot.
    pub fn latest(&self) -> Option<&ServerMetrics> {
        self.history.last()
    }

    /// Get average CPU usage over the history.
    pub fn average_cpu(&self) -> f32 {
        if self.history.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.history.iter().map(|m| m.cpu_percent).sum();
        sum / self.history.len() as f32
    }

    /// Get average memory usage over the history.
    pub fn average_memory_mb(&self) -> f32 {
        if self.history.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.history.iter().map(|m| m.memory_mb).sum();
        sum / self.history.len() as f32
    }
}
