//! # Hot Reload
//!
//! Script hot-reloading for live physics updates.
//!
//! ## Usage
//!
//! Scripts are automatically reloaded when modified on disk,
//! allowing real-time iteration on physics behaviors.

use bevy::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

/// Script manager resource
#[derive(Resource, Default)]
pub struct ScriptManager {
    /// Loaded scripts by path
    scripts: HashMap<PathBuf, LoadedScript>,
    /// Watch paths for hot-reload
    watch_paths: Vec<PathBuf>,
    /// Hot-reload enabled
    pub hot_reload_enabled: bool,
    /// Check interval (seconds)
    pub check_interval: f32,
    /// Time since last check
    last_check: f32,
}

/// A loaded script
#[derive(Clone)]
pub struct LoadedScript {
    /// Script path
    pub path: PathBuf,
    /// Script source code
    pub source: String,
    /// Last modification time
    pub last_modified: SystemTime,
    /// Is script valid/compiled
    pub is_valid: bool,
    /// Error message if invalid
    pub error: Option<String>,
}

impl ScriptManager {
    /// Create new script manager
    pub fn new() -> Self {
        Self {
            hot_reload_enabled: true,
            check_interval: 1.0,
            ..default()
        }
    }
    
    /// Add a watch path for hot-reload
    pub fn watch(&mut self, path: PathBuf) {
        if !self.watch_paths.contains(&path) {
            self.watch_paths.push(path);
        }
    }
    
    /// Load a script from path
    pub fn load(&mut self, path: &PathBuf) -> Result<(), String> {
        let source = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read script: {}", e))?;
        
        let metadata = std::fs::metadata(path)
            .map_err(|e| format!("Failed to get metadata: {}", e))?;
        
        let last_modified = metadata.modified()
            .unwrap_or(SystemTime::now());
        
        // Validate script (placeholder - actual validation with Rune)
        let (is_valid, error) = self.validate_script(&source);
        
        self.scripts.insert(path.clone(), LoadedScript {
            path: path.clone(),
            source,
            last_modified,
            is_valid,
            error,
        });
        
        if is_valid {
            info!("Loaded script: {:?}", path);
        } else {
            warn!("Script has errors: {:?}", path);
        }
        
        Ok(())
    }
    
    /// Reload a script if modified
    pub fn reload_if_modified(&mut self, path: &PathBuf) -> bool {
        let current_modified = match std::fs::metadata(path) {
            Ok(m) => m.modified().unwrap_or(SystemTime::now()),
            Err(_) => return false,
        };
        
        let needs_reload = self.scripts.get(path)
            .map(|s| s.last_modified < current_modified)
            .unwrap_or(true);
        
        if needs_reload {
            if let Err(e) = self.load(path) {
                warn!("Failed to reload script {:?}: {}", path, e);
                return false;
            }
            info!("Hot-reloaded script: {:?}", path);
            return true;
        }
        
        false
    }
    
    /// Check all watched paths for changes
    pub fn check_all(&mut self) -> Vec<PathBuf> {
        let mut reloaded = Vec::new();
        
        for path in self.watch_paths.clone() {
            if self.reload_if_modified(&path) {
                reloaded.push(path);
            }
        }
        
        reloaded
    }
    
    /// Get a loaded script
    pub fn get(&self, path: &PathBuf) -> Option<&LoadedScript> {
        self.scripts.get(path)
    }
    
    /// Get all loaded scripts
    pub fn all_scripts(&self) -> impl Iterator<Item = &LoadedScript> {
        self.scripts.values()
    }
    
    /// Validate script syntax (placeholder)
    fn validate_script(&self, source: &str) -> (bool, Option<String>) {
        // Basic validation - check for common issues
        if source.trim().is_empty() {
            return (false, Some("Empty script".to_string()));
        }
        
        // Check for balanced braces
        let open_braces = source.matches('{').count();
        let close_braces = source.matches('}').count();
        if open_braces != close_braces {
            return (false, Some("Unbalanced braces".to_string()));
        }
        
        // Check for balanced parentheses
        let open_parens = source.matches('(').count();
        let close_parens = source.matches(')').count();
        if open_parens != close_parens {
            return (false, Some("Unbalanced parentheses".to_string()));
        }
        
        // Full validation would use Rune's compiler
        (true, None)
    }
    
    /// Unload a script
    pub fn unload(&mut self, path: &PathBuf) {
        self.scripts.remove(path);
        self.watch_paths.retain(|p| p != path);
    }
    
    /// Clear all scripts
    pub fn clear(&mut self) {
        self.scripts.clear();
        self.watch_paths.clear();
    }
}

/// System to check for script changes
pub fn check_script_changes(
    mut manager: ResMut<ScriptManager>,
    time: Res<Time>,
) {
    if !manager.hot_reload_enabled {
        return;
    }
    
    manager.last_check += time.delta_secs();
    
    if manager.last_check >= manager.check_interval {
        manager.last_check = 0.0;
        let reloaded = manager.check_all();
        
        for path in reloaded {
            info!("Script reloaded: {:?}", path);
        }
    }
}

/// Event sent when a script is reloaded
#[derive(Event, Clone)]
pub struct ScriptReloadedEvent {
    /// Path to the reloaded script
    pub path: PathBuf,
    /// Whether the script is valid
    pub is_valid: bool,
}

/// Example Rune script template
pub const EXAMPLE_PHYSICS_SCRIPT: &str = r#"// Example physics script
// This script demonstrates the Rune API for physics

use physics::{ideal_gas_pressure, kinetic_energy, drag_force};
use entity::{get_temperature, set_temperature, apply_force};

/// Called every frame for each physics entity
pub fn update(entity_id, dt) {
    // Get current temperature
    let temp = entity::get_temperature(entity_id);
    
    // Calculate pressure using ideal gas law
    let pressure = physics::ideal_gas_pressure(1.0, temp, 0.001);
    
    // Apply force if pressure exceeds threshold
    if pressure > 200000.0 {
        let force = (pressure - 200000.0) * 0.001;
        entity::apply_force(entity_id, 0.0, force, 0.0);
    }
}

/// Custom thermal expansion behavior
pub fn thermal_expansion(entity_id, heat_input) {
    let current_temp = entity::get_temperature(entity_id);
    let new_temp = current_temp + heat_input / 1000.0;
    entity::set_temperature(entity_id, new_temp);
}

/// Aerodynamic drag calculation
pub fn apply_drag(entity_id, velocity, density) {
    let speed = (velocity.x * velocity.x + velocity.y * velocity.y + velocity.z * velocity.z).sqrt();
    let drag = physics::drag_force(density, speed, 0.47, 1.0);
    
    if speed > 0.01 {
        let drag_x = -drag * velocity.x / speed;
        let drag_y = -drag * velocity.y / speed;
        let drag_z = -drag * velocity.z / speed;
        entity::apply_force(entity_id, drag_x, drag_y, drag_z);
    }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_script_validation() {
        let manager = ScriptManager::new();
        
        // Valid script
        let (valid, _) = manager.validate_script("fn test() { }");
        assert!(valid);
        
        // Empty script
        let (valid, error) = manager.validate_script("");
        assert!(!valid);
        assert!(error.is_some());
        
        // Unbalanced braces
        let (valid, error) = manager.validate_script("fn test() {");
        assert!(!valid);
        assert!(error.unwrap().contains("braces"));
    }
}
