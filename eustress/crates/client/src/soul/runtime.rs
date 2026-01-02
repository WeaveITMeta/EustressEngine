//! # Soul Runtime
//!
//! Executes compiled Soul scripts at runtime.

use bevy::prelude::*;
use std::collections::HashMap;

use super::ExecutionContext;
use eustress_common::soul::{ScriptService, ScriptType};

// ============================================================================
// Soul Runtime
// ============================================================================

/// Soul script runtime executor
#[derive(Resource, Default)]
pub struct SoulRuntime {
    /// Loaded script modules
    modules: HashMap<String, LoadedModule>,
    /// Execution statistics
    stats: RuntimeStats,
    /// Error log
    errors: Vec<RuntimeError>,
}

/// A loaded script module
#[derive(Debug, Clone)]
pub struct LoadedModule {
    /// Module ID
    pub id: String,
    /// Scene name
    pub scene: String,
    /// Target service
    pub service: ScriptService,
    /// Script type
    pub script_type: ScriptType,
    /// Is module active?
    pub active: bool,
    /// System handles (for Bevy)
    pub systems: Vec<String>,
    /// Load timestamp
    pub loaded_at: std::time::Instant,
}

/// Runtime statistics
#[derive(Debug, Clone, Default)]
pub struct RuntimeStats {
    /// Total executions
    pub total_executions: u64,
    /// Successful executions
    pub successful_executions: u64,
    /// Failed executions
    pub failed_executions: u64,
    /// Total execution time (ms)
    pub total_time_ms: u64,
    /// Average execution time (ms)
    pub avg_time_ms: f64,
}

/// Runtime error
#[derive(Debug, Clone)]
pub struct RuntimeError {
    /// Script ID
    pub script_id: String,
    /// Error message
    pub message: String,
    /// Timestamp
    pub timestamp: std::time::Instant,
    /// Stack trace (if available)
    pub stack_trace: Option<String>,
}

impl SoulRuntime {
    /// Load a module
    pub fn load_module(&mut self, module: LoadedModule) {
        self.modules.insert(module.id.clone(), module);
    }
    
    /// Unload a module
    pub fn unload_module(&mut self, id: &str) -> Option<LoadedModule> {
        self.modules.remove(id)
    }
    
    /// Get a loaded module
    pub fn get_module(&self, id: &str) -> Option<&LoadedModule> {
        self.modules.get(id)
    }
    
    /// Execute a script
    pub fn execute(&mut self, script_id: &str, context: &ExecutionContext) {
        let start = std::time::Instant::now();
        
        self.stats.total_executions += 1;
        
        // Find the module
        let module = match self.modules.get(script_id) {
            Some(m) if m.active => m,
            Some(_) => {
                self.record_error(script_id, "Module is not active");
                return;
            }
            None => {
                self.record_error(script_id, "Module not found");
                return;
            }
        };
        
        // Execute (placeholder - in real impl, this would call the compiled systems)
        // The actual execution happens through Bevy's system scheduling
        
        let duration = start.elapsed().as_millis() as u64;
        self.stats.successful_executions += 1;
        self.stats.total_time_ms += duration;
        self.stats.avg_time_ms = self.stats.total_time_ms as f64 / self.stats.successful_executions as f64;
    }
    
    /// Record an error
    fn record_error(&mut self, script_id: &str, message: &str) {
        self.stats.failed_executions += 1;
        self.errors.push(RuntimeError {
            script_id: script_id.to_string(),
            message: message.to_string(),
            timestamp: std::time::Instant::now(),
            stack_trace: None,
        });
        
        // Keep only last 100 errors
        if self.errors.len() > 100 {
            self.errors.remove(0);
        }
    }
    
    /// Get runtime statistics
    pub fn stats(&self) -> &RuntimeStats {
        &self.stats
    }
    
    /// Get recent errors
    pub fn recent_errors(&self, count: usize) -> &[RuntimeError] {
        let start = self.errors.len().saturating_sub(count);
        &self.errors[start..]
    }
    
    /// Clear errors
    pub fn clear_errors(&mut self) {
        self.errors.clear();
    }
    
    /// Get all loaded modules
    pub fn modules(&self) -> impl Iterator<Item = &LoadedModule> {
        self.modules.values()
    }
    
    /// Get modules for a scene
    pub fn modules_for_scene(&self, scene: &str) -> Vec<&LoadedModule> {
        self.modules.values()
            .filter(|m| m.scene == scene)
            .collect()
    }
    
    /// Get modules for a service
    pub fn modules_for_service(&self, service: ScriptService) -> Vec<&LoadedModule> {
        self.modules.values()
            .filter(|m| m.service == service)
            .collect()
    }
    
    /// Activate a module
    pub fn activate(&mut self, id: &str) -> bool {
        if let Some(module) = self.modules.get_mut(id) {
            module.active = true;
            true
        } else {
            false
        }
    }
    
    /// Deactivate a module
    pub fn deactivate(&mut self, id: &str) -> bool {
        if let Some(module) = self.modules.get_mut(id) {
            module.active = false;
            true
        } else {
            false
        }
    }
    
    /// Reload a module (deactivate, unload, load, activate)
    pub fn reload(&mut self, id: &str, new_module: LoadedModule) {
        self.deactivate(id);
        self.unload_module(id);
        self.load_module(new_module);
        self.activate(id);
    }
}

// ============================================================================
// Script Trait
// ============================================================================

/// Trait for Soul-generated scripts
pub trait SoulScript: Send + Sync {
    /// Get script ID
    fn id(&self) -> &str;
    
    /// Get target service
    fn service(&self) -> ScriptService;
    
    /// Get script type
    fn script_type(&self) -> ScriptType;
    
    /// Initialize the script
    fn init(&mut self, world: &mut World);
    
    /// Update the script (called each frame)
    fn update(&mut self, world: &mut World);
    
    /// Cleanup the script
    fn cleanup(&mut self, world: &mut World);
}

/// Boxed script for dynamic dispatch
pub type BoxedScript = Box<dyn SoulScript>;
