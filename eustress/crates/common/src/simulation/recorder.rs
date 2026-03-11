//! # Data Recorder
//!
//! Time-series data collection and export for simulation analysis.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// A complete simulation recording
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimulationRecording {
    /// Recording metadata
    pub metadata: RecordingMetadata,
    
    /// Time series data by variable name
    pub series: HashMap<String, TimeSeries>,
    
    /// Events that occurred during simulation
    pub events: Vec<SimulationEvent>,
}

/// Metadata about the recording
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecordingMetadata {
    /// Recording name/identifier
    pub name: String,
    
    /// When recording started (wall clock)
    pub started_at: String,
    
    /// Total simulation time covered
    pub simulation_duration_s: f64,
    
    /// Total wall time elapsed
    pub wall_duration_s: f64,
    
    /// Total ticks recorded
    pub total_ticks: u64,
    
    /// Effective time compression ratio
    pub compression_ratio: f64,
    
    /// Additional metadata
    pub tags: HashMap<String, String>,
}

impl Default for RecordingMetadata {
    fn default() -> Self {
        Self {
            name: "Untitled Recording".to_string(),
            started_at: String::new(),
            simulation_duration_s: 0.0,
            wall_duration_s: 0.0,
            total_ticks: 0,
            compression_ratio: 1.0,
            tags: HashMap::new(),
        }
    }
}

/// A time series of values
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeSeries {
    /// Variable name
    pub name: String,
    
    /// Display label
    pub label: String,
    
    /// Unit string
    pub unit: String,
    
    /// Time values (simulation seconds)
    pub times: Vec<f64>,
    
    /// Data values
    pub values: Vec<f64>,
    
    /// Statistics
    pub stats: TimeSeriesStats,
}

/// Statistics for a time series
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimeSeriesStats {
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub std_dev: f64,
    pub first: f64,
    pub last: f64,
}

impl TimeSeries {
    /// Create new time series
    pub fn new(name: &str, label: &str, unit: &str) -> Self {
        Self {
            name: name.to_string(),
            label: label.to_string(),
            unit: unit.to_string(),
            times: Vec::new(),
            values: Vec::new(),
            stats: TimeSeriesStats::default(),
        }
    }
    
    /// Add a data point
    pub fn push(&mut self, time: f64, value: f64) {
        self.times.push(time);
        self.values.push(value);
    }
    
    /// Calculate statistics
    pub fn compute_stats(&mut self) {
        if self.values.is_empty() {
            return;
        }
        
        let n = self.values.len() as f64;
        let sum: f64 = self.values.iter().sum();
        let mean = sum / n;
        
        let variance: f64 = self.values.iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f64>() / n;
        
        self.stats = TimeSeriesStats {
            min: self.values.iter().cloned().fold(f64::INFINITY, f64::min),
            max: self.values.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            mean,
            std_dev: variance.sqrt(),
            first: self.values.first().copied().unwrap_or(0.0),
            last: self.values.last().copied().unwrap_or(0.0),
        };
    }
}

/// An event that occurred during simulation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimulationEvent {
    /// Simulation time when event occurred
    pub time_s: f64,
    
    /// Tick when event occurred
    pub tick: u64,
    
    /// Event type/category
    pub event_type: String,
    
    /// Event description
    pub description: String,
    
    /// Associated data
    pub data: HashMap<String, f64>,
}

impl SimulationRecording {
    /// Create new recording
    pub fn new(name: &str) -> Self {
        Self {
            metadata: RecordingMetadata {
                name: name.to_string(),
                ..default()
            },
            series: HashMap::new(),
            events: Vec::new(),
        }
    }
    
    /// Add or update a time series
    pub fn add_series(&mut self, series: TimeSeries) {
        self.series.insert(series.name.clone(), series);
    }
    
    /// Record an event
    pub fn add_event(&mut self, event: SimulationEvent) {
        self.events.push(event);
    }
    
    /// Finalize recording and compute statistics
    pub fn finalize(&mut self) {
        for series in self.series.values_mut() {
            series.compute_stats();
        }
        
        if self.metadata.wall_duration_s > 0.0 {
            self.metadata.compression_ratio = 
                self.metadata.simulation_duration_s / self.metadata.wall_duration_s;
        }
    }
    
    /// Export to JSON file
    pub fn export_json(&self, path: &Path) -> std::io::Result<()> {
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }
    
    /// Export to CSV file (one file per series)
    pub fn export_csv(&self, directory: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(directory)?;
        
        for (name, series) in &self.series {
            let filename = format!("{}.csv", name.replace(['/', '\\', ':'], "_"));
            let path = directory.join(filename);
            let mut file = File::create(path)?;
            
            writeln!(file, "time_s,{}", name)?;
            for (t, v) in series.times.iter().zip(series.values.iter()) {
                writeln!(file, "{},{}", t, v)?;
            }
        }
        
        let events_path = directory.join("events.csv");
        let mut events_file = File::create(events_path)?;
        writeln!(events_file, "time_s,tick,event_type,description")?;
        for event in &self.events {
            writeln!(events_file, "{},{},{},\"{}\"", 
                event.time_s, event.tick, event.event_type, event.description)?;
        }
        
        Ok(())
    }
    
    /// Generate summary report as string
    pub fn summary(&self) -> String {
        let mut report = String::new();
        
        report.push_str(&format!("# Simulation Report: {}\n\n", self.metadata.name));
        report.push_str("## Summary\n");
        report.push_str(&format!("- Simulation Duration: {:.2} s ({:.2} hours)\n", 
            self.metadata.simulation_duration_s,
            self.metadata.simulation_duration_s / 3600.0));
        report.push_str(&format!("- Wall Time: {:.2} s\n", self.metadata.wall_duration_s));
        report.push_str(&format!("- Compression Ratio: {:.0}x\n", self.metadata.compression_ratio));
        report.push_str(&format!("- Total Ticks: {}\n\n", self.metadata.total_ticks));
        
        report.push_str("## Variables\n");
        for (name, series) in &self.series {
            report.push_str(&format!("### {} ({})\n", series.label, series.unit));
            report.push_str(&format!("- Min: {:.4}\n", series.stats.min));
            report.push_str(&format!("- Max: {:.4}\n", series.stats.max));
            report.push_str(&format!("- Mean: {:.4}\n", series.stats.mean));
            report.push_str(&format!("- Std Dev: {:.4}\n", series.stats.std_dev));
            report.push_str(&format!("- Start → End: {:.4} → {:.4}\n\n", 
                series.stats.first, series.stats.last));
        }
        
        if !self.events.is_empty() {
            report.push_str("## Events\n");
            for event in &self.events {
                report.push_str(&format!("- [{:.2}s] {}: {}\n", 
                    event.time_s, event.event_type, event.description));
            }
        }
        
        report
    }
}

fn default<T: Default>() -> T {
    T::default()
}
