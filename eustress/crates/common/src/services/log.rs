//! # Log Service
//!
//! Logging and analytics.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Log service resource
#[derive(Resource, Default, Clone, Debug)]
pub struct LogService {
    pub entries: Vec<LogEntry>,
    pub max_entries: usize,
}

impl LogService {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries,
        }
    }
    
    pub fn log(&mut self, level: LogLevel, message: String) {
        let entry = LogEntry {
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
            level,
            message,
        };
        self.entries.push(entry);
        if self.entries.len() > self.max_entries {
            self.entries.remove(0);
        }
    }
}

/// Log entry
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp_ms: i64,
    pub level: LogLevel,
    pub message: String,
}

/// Log level
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}
