//! # Plugin Registry
//!
//! Stores all registered plugins and their metadata.

#[allow(dead_code)]

use bevy::prelude::*;
use std::collections::HashMap;
use super::api::{PluginInfo, StudioPlugin};

/// Registry of all available plugins
#[derive(Resource, Default)]
pub struct PluginRegistry {
    /// Registered plugins
    plugins: HashMap<String, Box<dyn StudioPlugin>>,
    /// Plugin info cache
    info_cache: HashMap<String, PluginInfo>,
}

impl PluginRegistry {
    /// Create a new registry
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Register a plugin
    pub fn register<P: StudioPlugin + 'static>(&mut self, plugin: P) {
        let info = plugin.info();
        let id = info.id.clone();
        
        info!("🔌 Registering plugin: {} v{} by {}", info.name, info.version, info.author);
        
        self.info_cache.insert(id.clone(), info);
        self.plugins.insert(id, Box::new(plugin));
    }
    
    /// Unregister a plugin
    pub fn unregister(&mut self, plugin_id: &str) -> Option<Box<dyn StudioPlugin>> {
        self.info_cache.remove(plugin_id);
        self.plugins.remove(plugin_id)
    }
    
    /// Get a plugin by ID
    pub fn get(&self, plugin_id: &str) -> Option<&dyn StudioPlugin> {
        self.plugins.get(plugin_id).map(|p| p.as_ref())
    }
    
    /// Get a mutable plugin by ID
    pub fn get_mut(&mut self, plugin_id: &str) -> Option<&mut (dyn StudioPlugin + 'static)> {
        self.plugins.get_mut(plugin_id).map(|p| p.as_mut())
    }
    
    /// Get plugin info by ID
    pub fn get_info(&self, plugin_id: &str) -> Option<&PluginInfo> {
        self.info_cache.get(plugin_id)
    }
    
    /// Get all plugin IDs
    pub fn plugin_ids(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
    }
    
    /// Get all plugin info
    pub fn all_info(&self) -> Vec<&PluginInfo> {
        self.info_cache.values().collect()
    }
    
    /// Count registered plugins
    pub fn count(&self) -> usize {
        self.plugins.len()
    }
    
    /// Check if a plugin is registered
    pub fn contains(&self, plugin_id: &str) -> bool {
        self.plugins.contains_key(plugin_id)
    }
}
