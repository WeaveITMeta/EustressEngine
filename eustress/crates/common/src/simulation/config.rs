//! # Simulation Configuration
//!
//! TOML-loadable configuration for simulation parameters.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Root simulation configuration loaded from TOML
#[derive(Clone, Debug, Serialize, Deserialize, Resource)]
pub struct SimulationConfig {
    /// General simulation settings
    #[serde(default)]
    pub simulation: SimulationSettings,
    
    /// Watchpoint definitions
    #[serde(default)]
    pub watchpoints: Vec<WatchPointConfig>,
    
    /// Breakpoint definitions
    #[serde(default)]
    pub breakpoints: Vec<BreakPointConfig>,
    
    /// Test suite definitions
    #[serde(default)]
    pub tests: Vec<TestConfig>,
    
    /// Custom parameters accessible from Rune scripts
    #[serde(default)]
    pub parameters: HashMap<String, f64>,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            simulation: SimulationSettings::default(),
            watchpoints: Vec::new(),
            breakpoints: Vec::new(),
            tests: Vec::new(),
            parameters: HashMap::new(),
        }
    }
}

/// General simulation settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimulationSettings {
    /// Tick rate in Hz (default 60)
    #[serde(default = "default_tick_rate")]
    pub tick_rate_hz: f64,
    
    /// Time scale (1.0 = real-time)
    #[serde(default = "default_time_scale")]
    pub time_scale: f64,
    
    /// Maximum ticks per frame
    #[serde(default = "default_max_ticks")]
    pub max_ticks_per_frame: u32,
    
    /// Auto-start simulation on load
    #[serde(default)]
    pub auto_start: bool,
    
    /// Maximum simulation time (None = unlimited)
    pub max_simulation_time_s: Option<f64>,
    
    /// Maximum tick count (None = unlimited)
    pub max_ticks: Option<u64>,
    
    /// Recording settings
    #[serde(default)]
    pub recording: RecordingSettings,
}

impl Default for SimulationSettings {
    fn default() -> Self {
        Self {
            tick_rate_hz: 60.0,
            time_scale: 1.0,
            max_ticks_per_frame: 10,
            auto_start: false,
            max_simulation_time_s: None,
            max_ticks: None,
            recording: RecordingSettings::default(),
        }
    }
}

fn default_tick_rate() -> f64 { 60.0 }
fn default_time_scale() -> f64 { 1.0 }
fn default_max_ticks() -> u32 { 10 }

/// Recording settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecordingSettings {
    /// Enable recording
    #[serde(default)]
    pub enabled: bool,
    
    /// Output directory for recordings
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
    
    /// Export format
    #[serde(default)]
    pub format: ExportFormat,
    
    /// Auto-export on simulation complete
    #[serde(default)]
    pub auto_export: bool,
}

impl Default for RecordingSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            output_dir: "recordings".to_string(),
            format: ExportFormat::Json,
            auto_export: false,
        }
    }
}

fn default_output_dir() -> String { "recordings".to_string() }

/// Export format for recordings
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    #[default]
    Json,
    Csv,
    Both,
}

/// Watchpoint configuration from TOML
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WatchPointConfig {
    /// Unique name
    pub name: String,
    
    /// Display label
    #[serde(default)]
    pub label: Option<String>,
    
    /// Unit string
    #[serde(default)]
    pub unit: String,
    
    /// Recording interval in ticks
    #[serde(default = "default_interval")]
    pub interval: u32,
    
    /// Maximum history size
    #[serde(default = "default_history")]
    pub max_history: usize,
    
    /// Display color (hex string like "#FF0000")
    #[serde(default)]
    pub color: Option<String>,
    
    /// Whether enabled by default
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_interval() -> u32 { 1 }
fn default_history() -> usize { 10000 }
fn default_true() -> bool { true }

/// Breakpoint configuration from TOML
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BreakPointConfig {
    /// Unique name
    pub name: String,
    
    /// Variable to watch
    pub variable: String,
    
    /// Comparison operator ("<", "<=", "==", ">=", ">", "!=")
    pub comparison: String,
    
    /// Threshold value
    pub threshold: f64,
    
    /// Description
    #[serde(default)]
    pub description: Option<String>,
    
    /// One-shot (disable after first trigger)
    #[serde(default)]
    pub one_shot: bool,
    
    /// Cooldown ticks between triggers
    #[serde(default)]
    pub cooldown: u32,
    
    /// Whether enabled by default
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Test configuration from TOML
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TestConfig {
    /// Test name
    pub name: String,
    
    /// Test description
    #[serde(default)]
    pub description: String,
    
    /// Rune script to execute
    pub script: String,
    
    /// Time scale for this test
    #[serde(default = "default_time_scale")]
    pub time_scale: f64,
    
    /// Maximum simulation time for test
    pub max_time_s: Option<f64>,
    
    /// Expected results for validation
    #[serde(default)]
    pub expected: HashMap<String, ExpectedValue>,
}

/// Expected value for test validation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExpectedValue {
    /// Minimum acceptable value
    pub min: Option<f64>,
    
    /// Maximum acceptable value
    pub max: Option<f64>,
    
    /// Exact expected value (with tolerance)
    pub value: Option<f64>,
    
    /// Tolerance for exact value comparison
    #[serde(default = "default_tolerance")]
    pub tolerance: f64,
}

fn default_tolerance() -> f64 { 0.001 }

impl SimulationConfig {
    /// Load configuration from TOML file
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::IoError(e.to_string()))?;
        
        toml::from_str(&content)
            .map_err(|e| ConfigError::ParseError(e.to_string()))
    }
    
    /// Save configuration to TOML file
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| ConfigError::SerializeError(e.to_string()))?;
        
        std::fs::write(path, content)
            .map_err(|e| ConfigError::IoError(e.to_string()))
    }
    
    /// Parse hex color string to RGBA bytes
    pub fn parse_color(hex: &str) -> Option<[u8; 4]> {
        let hex = hex.trim_start_matches('#');
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some([r, g, b, 255])
        } else if hex.len() == 8 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some([r, g, b, a])
        } else {
            None
        }
    }
}

/// Configuration loading errors
#[derive(Debug, Clone)]
pub enum ConfigError {
    IoError(String),
    ParseError(String),
    SerializeError(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::IoError(e) => write!(f, "IO error: {}", e),
            ConfigError::ParseError(e) => write!(f, "Parse error: {}", e),
            ConfigError::SerializeError(e) => write!(f, "Serialize error: {}", e),
        }
    }
}

impl std::error::Error for ConfigError {}
