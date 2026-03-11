//! # BreakPoint System
//!
//! Conditional simulation pause when conditions are met.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Comparison operator for breakpoint conditions
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Comparison {
    LessThan,
    LessOrEqual,
    Equal,
    GreaterOrEqual,
    GreaterThan,
    NotEqual,
}

impl Comparison {
    /// Evaluate comparison
    pub fn evaluate(&self, left: f64, right: f64) -> bool {
        match self {
            Comparison::LessThan => left < right,
            Comparison::LessOrEqual => left <= right,
            Comparison::Equal => (left - right).abs() < f64::EPSILON,
            Comparison::GreaterOrEqual => left >= right,
            Comparison::GreaterThan => left > right,
            Comparison::NotEqual => (left - right).abs() >= f64::EPSILON,
        }
    }
    
    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim() {
            "<" => Some(Comparison::LessThan),
            "<=" => Some(Comparison::LessOrEqual),
            "==" | "=" => Some(Comparison::Equal),
            ">=" => Some(Comparison::GreaterOrEqual),
            ">" => Some(Comparison::GreaterThan),
            "!=" | "<>" => Some(Comparison::NotEqual),
            _ => None,
        }
    }
}

/// A breakpoint pauses simulation when condition is met
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BreakPoint {
    /// Unique identifier
    pub name: String,
    
    /// Human-readable description
    pub description: String,
    
    /// Variable name to watch (must exist in WatchPointRegistry)
    pub variable: String,
    
    /// Comparison operator
    pub comparison: Comparison,
    
    /// Threshold value
    pub threshold: f64,
    
    /// Whether breakpoint is enabled
    pub enabled: bool,
    
    /// Number of times this breakpoint has triggered
    pub hit_count: u32,
    
    /// Whether to auto-disable after first hit
    pub one_shot: bool,
    
    /// Minimum ticks between triggers (debounce)
    pub cooldown_ticks: u32,
    
    /// Ticks since last trigger
    ticks_since_trigger: u32,
}

impl BreakPoint {
    /// Create a new breakpoint
    pub fn new(name: &str, variable: &str, comparison: Comparison, threshold: f64) -> Self {
        Self {
            name: name.to_string(),
            description: format!("{} {} {}", variable, 
                match comparison {
                    Comparison::LessThan => "<",
                    Comparison::LessOrEqual => "<=",
                    Comparison::Equal => "==",
                    Comparison::GreaterOrEqual => ">=",
                    Comparison::GreaterThan => ">",
                    Comparison::NotEqual => "!=",
                },
                threshold
            ),
            variable: variable.to_string(),
            comparison,
            threshold,
            enabled: true,
            hit_count: 0,
            one_shot: false,
            cooldown_ticks: 0,
            ticks_since_trigger: u32::MAX,
        }
    }
    
    /// Create with description
    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }
    
    /// Create as one-shot (disables after first trigger)
    pub fn one_shot(mut self) -> Self {
        self.one_shot = true;
        self
    }
    
    /// Create with cooldown
    pub fn with_cooldown(mut self, ticks: u32) -> Self {
        self.cooldown_ticks = ticks;
        self
    }
    
    /// Check if condition is met and should trigger
    pub fn check(&mut self, value: f64) -> bool {
        if !self.enabled {
            return false;
        }
        
        self.ticks_since_trigger = self.ticks_since_trigger.saturating_add(1);
        
        if self.ticks_since_trigger < self.cooldown_ticks {
            return false;
        }
        
        if self.comparison.evaluate(value, self.threshold) {
            self.hit_count += 1;
            self.ticks_since_trigger = 0;
            
            if self.one_shot {
                self.enabled = false;
            }
            
            return true;
        }
        
        false
    }
    
    /// Reset breakpoint state
    pub fn reset(&mut self) {
        self.hit_count = 0;
        self.ticks_since_trigger = u32::MAX;
        if self.one_shot {
            self.enabled = true;
        }
    }
}

/// Resource holding all active breakpoints
#[derive(Resource, Default, Clone, Debug)]
pub struct BreakPointRegistry {
    /// Breakpoints by name
    pub breakpoints: HashMap<String, BreakPoint>,
}

impl BreakPointRegistry {
    /// Register a new breakpoint
    pub fn register(&mut self, breakpoint: BreakPoint) {
        self.breakpoints.insert(breakpoint.name.clone(), breakpoint);
    }
    
    /// Remove a breakpoint
    pub fn remove(&mut self, name: &str) -> Option<BreakPoint> {
        self.breakpoints.remove(name)
    }
    
    /// Get breakpoint by name
    pub fn get(&self, name: &str) -> Option<&BreakPoint> {
        self.breakpoints.get(name)
    }
    
    /// Get mutable breakpoint
    pub fn get_mut(&mut self, name: &str) -> Option<&mut BreakPoint> {
        self.breakpoints.get_mut(name)
    }
    
    /// Enable/disable breakpoint
    pub fn set_enabled(&mut self, name: &str, enabled: bool) {
        if let Some(bp) = self.breakpoints.get_mut(name) {
            bp.enabled = enabled;
        }
    }
    
    /// Check all breakpoints against current values, returns names of triggered breakpoints
    pub fn check_all(&mut self, values: &HashMap<String, f64>) -> Vec<String> {
        let mut triggered = Vec::new();
        
        for (name, bp) in self.breakpoints.iter_mut() {
            if let Some(&value) = values.get(&bp.variable) {
                if bp.check(value) {
                    triggered.push(name.clone());
                }
            }
        }
        
        triggered
    }
    
    /// Reset all breakpoints
    pub fn reset_all(&mut self) {
        for bp in self.breakpoints.values_mut() {
            bp.reset();
        }
    }
    
    /// Get all breakpoint names
    pub fn names(&self) -> Vec<&str> {
        self.breakpoints.keys().map(|s| s.as_str()).collect()
    }
}
