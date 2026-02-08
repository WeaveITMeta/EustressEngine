//! # Plugin Manager
//!
//! Manages the lifecycle of Studio plugins - loading, enabling, disabling, and unloading.

#[allow(dead_code)]

use bevy::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use super::api::{PluginApi, StudioPlugin, PluginAction, PluginNotification, SimClock, ScheduledEvent};
use super::registry::PluginRegistry;

/// Manages all loaded plugins
#[derive(Resource, Default)]
pub struct PluginManager {
    /// Plugin APIs indexed by plugin ID
    apis: HashMap<String, PluginApi>,
    /// Enabled plugin IDs
    enabled: Vec<String>,
    /// Plugin directory
    plugins_dir: Option<PathBuf>,
}

impl PluginManager {
    /// Create a new plugin manager
    pub fn new() -> Self {
        let plugins_dir = dirs::data_local_dir()
            .map(|d| d.join("Eustress").join("Plugins"));
        
        // Create plugins directory if it doesn't exist
        if let Some(ref dir) = plugins_dir {
            let _ = std::fs::create_dir_all(dir);
        }
        
        Self {
            apis: HashMap::new(),
            enabled: Vec::new(),
            plugins_dir,
        }
    }
    
    /// Get the plugins directory
    pub fn plugins_dir(&self) -> Option<&PathBuf> {
        self.plugins_dir.as_ref()
    }
    
    /// Open the plugins directory in the system file manager
    pub fn open_plugins_dir(&self) -> Result<(), String> {
        if let Some(ref dir) = self.plugins_dir {
            // Create if doesn't exist
            let _ = std::fs::create_dir_all(dir);
            
            #[cfg(target_os = "windows")]
            {
                std::process::Command::new("explorer")
                    .arg(dir)
                    .spawn()
                    .map_err(|e| e.to_string())?;
            }
            
            #[cfg(target_os = "macos")]
            {
                std::process::Command::new("open")
                    .arg(dir)
                    .spawn()
                    .map_err(|e| e.to_string())?;
            }
            
            #[cfg(target_os = "linux")]
            {
                std::process::Command::new("xdg-open")
                    .arg(dir)
                    .spawn()
                    .map_err(|e| e.to_string())?;
            }
            
            Ok(())
        } else {
            Err("Plugins directory not set".to_string())
        }
    }
    
    /// Enable a plugin
    pub fn enable_plugin(&mut self, registry: &mut PluginRegistry, plugin_id: &str) -> Result<(), String> {
        if self.enabled.contains(&plugin_id.to_string()) {
            return Ok(()); // Already enabled
        }
        
        // Get the plugin from registry
        let plugin = registry.get_mut(plugin_id)
            .ok_or_else(|| format!("Plugin '{}' not found", plugin_id))?;
        
        // Create API for this plugin
        let mut api = PluginApi::new();
        
        // Call on_enable
        plugin.on_enable(&mut api);
        
        // Store API and mark as enabled
        self.apis.insert(plugin_id.to_string(), api);
        self.enabled.push(plugin_id.to_string());
        
        info!("🔌 Enabled plugin: {}", plugin_id);
        Ok(())
    }
    
    /// Disable a plugin
    pub fn disable_plugin(&mut self, registry: &mut PluginRegistry, plugin_id: &str) -> Result<(), String> {
        if !self.enabled.contains(&plugin_id.to_string()) {
            return Ok(()); // Already disabled
        }
        
        // Get the plugin from registry
        if let Some(plugin) = registry.get_mut(plugin_id) {
            if let Some(api) = self.apis.get_mut(plugin_id) {
                plugin.on_disable(api);
            }
        }
        
        // Remove API and mark as disabled
        self.apis.remove(plugin_id);
        self.enabled.retain(|id| id != plugin_id);
        
        info!("🔌 Disabled plugin: {}", plugin_id);
        Ok(())
    }
    
    /// Check if a plugin is enabled
    pub fn is_enabled(&self, plugin_id: &str) -> bool {
        self.enabled.contains(&plugin_id.to_string())
    }
    
    /// Get list of enabled plugins
    pub fn enabled_plugins(&self) -> &[String] {
        &self.enabled
    }
    
    /// Get API for a plugin
    pub fn get_api(&self, plugin_id: &str) -> Option<&PluginApi> {
        self.apis.get(plugin_id)
    }
    
    /// Get mutable API for a plugin
    pub fn get_api_mut(&mut self, plugin_id: &str) -> Option<&mut PluginApi> {
        self.apis.get_mut(plugin_id)
    }
    
    /// Tick all enabled plugins with simulation clock
    pub fn tick_with_clock(&mut self, registry: &mut PluginRegistry, clock: &SimClock) {
        // Process scheduled events first
        self.process_scheduled_events(registry, clock);
        
        // Then call on_update for all plugins
        for plugin_id in &self.enabled.clone() {
            if let (Some(plugin), Some(api)) = (registry.get_mut(plugin_id), self.apis.get_mut(plugin_id)) {
                plugin.on_update(api, clock);
            }
        }
    }
    
    /// Legacy tick method (uses default clock)
    pub fn tick(&mut self, registry: &mut PluginRegistry) {
        let default_clock = SimClock::default();
        self.tick_with_clock(registry, &default_clock);
    }
    
    /// Process scheduled events and dispatch to plugins
    fn process_scheduled_events(&mut self, registry: &mut PluginRegistry, clock: &SimClock) {
        let current_time = clock.current;
        
        for plugin_id in &self.enabled.clone() {
            if let Some(api) = self.apis.get_mut(plugin_id) {
                // Convert relative delays to absolute times and collect ready events
                let mut ready_events = Vec::new();
                let mut remaining_events = Vec::new();
                
                for mut event in api.scheduled_events.drain(..) {
                    // If scheduled_time is relative (less than current), convert to absolute
                    if event.scheduled_time < current_time {
                        event.scheduled_time = current_time + event.scheduled_time;
                    }
                    
                    if event.scheduled_time <= current_time {
                        ready_events.push(event.clone());
                        
                        // Re-schedule if repeating
                        if event.repeating && event.repeat_interval > 0.0 {
                            let mut next_event = event;
                            next_event.scheduled_time = current_time + next_event.repeat_interval;
                            remaining_events.push(next_event);
                        }
                    } else {
                        remaining_events.push(event);
                    }
                }
                
                // Put remaining events back
                api.scheduled_events = remaining_events;
                
                // Store ready events for plugin to process
                api.ready_events.extend(ready_events);
            }
        }
        
        // Dispatch ready events to plugins
        for plugin_id in &self.enabled.clone() {
            let ready_events: Vec<ScheduledEvent> = if let Some(api) = self.apis.get_mut(plugin_id) {
                api.take_ready_events()
            } else {
                Vec::new()
            };
            
            for event in ready_events {
                if let (Some(plugin), Some(api)) = (registry.get_mut(plugin_id), self.apis.get_mut(plugin_id)) {
                    plugin.on_scheduled_event(&event, api, clock);
                }
            }
        }
    }
    
    /// Render UI for all enabled plugins
    /// Note: Plugin UI rendering is now handled via Slint integration
    pub fn render_ui(&mut self, _registry: &mut PluginRegistry) {
        // Plugin UI is now handled by Slint - see slint_ui.rs
    }
    
    /// Handle menu action for plugins
    pub fn handle_menu_action(&mut self, registry: &mut PluginRegistry, action_id: &str) {
        for plugin_id in &self.enabled.clone() {
            if let (Some(plugin), Some(api)) = (registry.get_mut(plugin_id), self.apis.get_mut(plugin_id)) {
                plugin.on_menu_action(action_id, api);
            }
        }
    }
    
    /// Notify plugins of selection change
    pub fn notify_selection_changed(&mut self, registry: &mut PluginRegistry, selected: &[Entity]) {
        for plugin_id in &self.enabled.clone() {
            if let (Some(plugin), Some(api)) = (registry.get_mut(plugin_id), self.apis.get_mut(plugin_id)) {
                api.selected_entities = selected.to_vec();
                plugin.on_selection_changed(selected, api);
            }
        }
    }
    
    /// Collect all pending actions from plugins
    pub fn collect_actions(&mut self) -> Vec<(String, PluginAction)> {
        let mut actions = Vec::new();
        
        for (plugin_id, api) in &mut self.apis {
            for action in api.pending_actions.drain(..) {
                actions.push((plugin_id.clone(), action));
            }
        }
        
        actions
    }
    
    /// Collect all notifications from plugins
    pub fn collect_notifications(&mut self) -> Vec<(String, PluginNotification)> {
        let mut notifications = Vec::new();
        
        for (plugin_id, api) in &mut self.apis {
            for notification in api.notifications.drain(..) {
                notifications.push((plugin_id.clone(), notification));
            }
        }
        
        notifications
    }
    
    /// Get all registered panels from enabled plugins
    pub fn get_all_panels(&self) -> Vec<(String, super::api::PluginPanel)> {
        let mut panels = Vec::new();
        
        for (plugin_id, api) in &self.apis {
            for panel in &api.panels {
                panels.push((plugin_id.clone(), panel.clone()));
            }
        }
        
        panels
    }
    
    /// Get all registered menu items from enabled plugins
    pub fn get_all_menu_items(&self) -> Vec<(String, super::api::PluginMenuItem)> {
        let mut items = Vec::new();
        
        for (plugin_id, api) in &self.apis {
            for item in &api.menu_items {
                items.push((plugin_id.clone(), item.clone()));
            }
        }
        
        items
    }
    
    /// Toggle panel visibility
    pub fn toggle_panel(&mut self, plugin_id: &str, panel_id: &str) {
        if let Some(api) = self.apis.get_mut(plugin_id) {
            for panel in &mut api.panels {
                if panel.id == panel_id {
                    panel.visible = !panel.visible;
                    break;
                }
            }
        }
    }
    
    /// Set panel visibility
    pub fn set_panel_visible(&mut self, plugin_id: &str, panel_id: &str, visible: bool) {
        if let Some(api) = self.apis.get_mut(plugin_id) {
            for panel in &mut api.panels {
                if panel.id == panel_id {
                    panel.visible = visible;
                    break;
                }
            }
        }
    }
    
    /// Collect all pending tab registrations from plugins
    pub fn collect_pending_tabs(&mut self) -> Vec<(
        Vec<super::api::PendingTabRegistration>,
        Vec<super::api::PendingSectionRegistration>,
        Vec<super::api::PendingButtonRegistration>,
    )> {
        let mut results = Vec::new();
        
        for (_plugin_id, api) in &mut self.apis {
            let tabs = std::mem::take(&mut api.pending_tabs);
            let sections = std::mem::take(&mut api.pending_sections);
            let buttons = std::mem::take(&mut api.pending_buttons);
            
            if !tabs.is_empty() || !sections.is_empty() || !buttons.is_empty() {
                results.push((tabs, sections, buttons));
            }
        }
        
        results
    }
}
