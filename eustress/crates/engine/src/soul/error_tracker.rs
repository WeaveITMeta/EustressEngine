//! # Rune Error Tracker
//!
//! Tracks and categorizes script errors for debugging.

use bevy::prelude::*;
use std::collections::HashMap;

/// Error category for classification
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ErrorCategory {
    Syntax,
    Runtime,
    Type,
    Reference,
    Api,
    Unknown,
}

/// A tracked error with context
#[derive(Debug, Clone)]
pub struct TrackedError {
    pub category: String,
    pub message: String,
    pub timestamp: std::time::Instant,
    pub context: Option<String>,
}

/// Suggested action for fixing an error
#[derive(Debug, Clone)]
pub struct SuggestedAction {
    pub description: String,
    pub code_fix: Option<String>,
}

/// Error statistics
#[derive(Debug, Clone, Default)]
pub struct ErrorStats {
    pub total_errors: u32,
    pub errors_by_category: HashMap<String, u32>,
    pub last_error_time: Option<std::time::Instant>,
    pub top_missing_functions: Vec<String>,
}

/// Resource for tracking Rune script errors
#[derive(Resource, Debug, Default)]
pub struct RuneErrorTracker {
    pub errors: Vec<TrackedError>,
    pub stats: ErrorStats,
    pub max_errors: usize,
}

impl RuneErrorTracker {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            stats: ErrorStats::default(),
            max_errors: 100,
        }
    }
    
    pub fn load(_path: &std::path::Path) -> Option<Self> {
        // TODO: Load from file
        Some(Self::new())
    }
    
    pub fn save(&self, _path: &std::path::Path) -> Result<(), std::io::Error> {
        // TODO: Save to file
        Ok(())
    }
    
    pub fn track_error(&mut self, error: TrackedError) {
        self.stats.total_errors += 1;
        *self.stats.errors_by_category.entry(error.category.clone()).or_insert(0) += 1;
        self.stats.last_error_time = Some(error.timestamp);
        
        self.errors.push(error);
        
        // Trim old errors
        while self.errors.len() > self.max_errors {
            self.errors.remove(0);
        }
    }
    
    pub fn clear(&mut self) {
        self.errors.clear();
        self.stats = ErrorStats::default();
    }
    
    pub fn suggest_fix(&self, _error: &TrackedError) -> Option<SuggestedAction> {
        // TODO: Implement error-specific suggestions
        None
    }
    
    pub fn generate_report(&self) -> String {
        format!("Total errors: {}", self.stats.total_errors)
    }
    
    pub fn get_stats(&self) -> &ErrorStats {
        &self.stats
    }
    
    pub fn get_implementation_candidates(&self) -> Vec<String> {
        Vec::new()
    }
    
    pub fn get_deterministic_fix(&self, _error: &str) -> Option<String> {
        None
    }
    
    pub fn apply_fix(_code: &str, _fix: &str) -> String {
        String::new()
    }
}
