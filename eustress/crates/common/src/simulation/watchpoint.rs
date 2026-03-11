//! # WatchPoint System
//!
//! Observable variable tracking with history for graphing and analysis.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Single data point in a watchpoint history
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataPoint {
    /// Simulation time when recorded
    pub time_s: f64,
    
    /// Tick count when recorded
    pub tick: u64,
    
    /// Recorded value
    pub value: f64,
}

/// A watchpoint tracks a named variable over time
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WatchPoint {
    /// Unique identifier for this watchpoint
    pub name: String,
    
    /// Human-readable label for UI display
    pub label: String,
    
    /// Unit string for display (e.g., "V", "A", "°C", "%")
    pub unit: String,
    
    /// History of recorded values
    pub history: VecDeque<DataPoint>,
    
    /// Maximum history size (oldest dropped when exceeded)
    pub max_history: usize,
    
    /// Current value
    pub current: f64,
    
    /// Minimum value seen
    pub min: f64,
    
    /// Maximum value seen
    pub max: f64,
    
    /// Running average
    pub average: f64,
    
    /// Sample count for average calculation
    sample_count: u64,
    
    /// Recording interval in ticks (1 = every tick, 10 = every 10th tick)
    pub record_interval: u32,
    
    /// Ticks since last record
    ticks_since_record: u32,
    
    /// Whether this watchpoint is enabled
    pub enabled: bool,
    
    /// Color for graphing (RGBA 0-255)
    pub color: [u8; 4],
}

impl WatchPoint {
    /// Create a new watchpoint
    pub fn new(name: &str, label: &str, unit: &str) -> Self {
        Self {
            name: name.to_string(),
            label: label.to_string(),
            unit: unit.to_string(),
            history: VecDeque::new(),
            max_history: 10000,
            current: 0.0,
            min: f64::MAX,
            max: f64::MIN,
            average: 0.0,
            sample_count: 0,
            record_interval: 1,
            ticks_since_record: 0,
            enabled: true,
            color: [255, 255, 255, 255],
        }
    }
    
    /// Create with custom history size
    pub fn with_history_size(mut self, size: usize) -> Self {
        self.max_history = size;
        self
    }
    
    /// Create with recording interval
    pub fn with_interval(mut self, interval: u32) -> Self {
        self.record_interval = interval.max(1);
        self
    }
    
    /// Create with color
    pub fn with_color(mut self, r: u8, g: u8, b: u8) -> Self {
        self.color = [r, g, b, 255];
        self
    }
    
    /// Record a value at given simulation time and tick
    pub fn record(&mut self, value: f64, time_s: f64, tick: u64) {
        if !self.enabled {
            return;
        }
        
        self.current = value;
        
        self.ticks_since_record += 1;
        if self.ticks_since_record < self.record_interval {
            return;
        }
        self.ticks_since_record = 0;
        
        if value < self.min { self.min = value; }
        if value > self.max { self.max = value; }
        
        self.sample_count += 1;
        self.average = self.average + (value - self.average) / self.sample_count as f64;
        
        self.history.push_back(DataPoint { time_s, tick, value });
        
        while self.history.len() > self.max_history {
            self.history.pop_front();
        }
    }
    
    /// Get history as vectors for graphing (time, value)
    pub fn get_graph_data(&self) -> (Vec<f64>, Vec<f64>) {
        let times: Vec<f64> = self.history.iter().map(|p| p.time_s).collect();
        let values: Vec<f64> = self.history.iter().map(|p| p.value).collect();
        (times, values)
    }
    
    /// Get recent N points
    pub fn recent(&self, n: usize) -> Vec<&DataPoint> {
        self.history.iter().rev().take(n).collect()
    }
    
    /// Clear history and reset statistics
    pub fn reset(&mut self) {
        self.history.clear();
        self.current = 0.0;
        self.min = f64::MAX;
        self.max = f64::MIN;
        self.average = 0.0;
        self.sample_count = 0;
        self.ticks_since_record = 0;
    }
}

/// Resource holding all active watchpoints
#[derive(Resource, Default, Clone, Debug)]
pub struct WatchPointRegistry {
    /// Watchpoints by name
    pub watchpoints: HashMap<String, WatchPoint>,
}

impl WatchPointRegistry {
    /// Register a new watchpoint
    pub fn register(&mut self, watchpoint: WatchPoint) {
        self.watchpoints.insert(watchpoint.name.clone(), watchpoint);
    }
    
    /// Get a watchpoint by name
    pub fn get(&self, name: &str) -> Option<&WatchPoint> {
        self.watchpoints.get(name)
    }
    
    /// Get mutable watchpoint by name
    pub fn get_mut(&mut self, name: &str) -> Option<&mut WatchPoint> {
        self.watchpoints.get_mut(name)
    }
    
    /// Record value to named watchpoint
    pub fn record(&mut self, name: &str, value: f64, time_s: f64, tick: u64) {
        if let Some(wp) = self.watchpoints.get_mut(name) {
            wp.record(value, time_s, tick);
        }
    }
    
    /// Get all watchpoint names
    pub fn names(&self) -> Vec<&str> {
        self.watchpoints.keys().map(|s| s.as_str()).collect()
    }
    
    /// Reset all watchpoints
    pub fn reset_all(&mut self) {
        for wp in self.watchpoints.values_mut() {
            wp.reset();
        }
    }
    
    /// Export all data to JSON-serializable format
    pub fn export(&self) -> HashMap<String, Vec<(f64, f64)>> {
        self.watchpoints.iter()
            .map(|(name, wp)| {
                let data: Vec<(f64, f64)> = wp.history.iter()
                    .map(|p| (p.time_s, p.value))
                    .collect();
                (name.clone(), data)
            })
            .collect()
    }
}
