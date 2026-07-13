//! # Tab API (Studio Plugins)
//!
//! Stub module for Tab API in studio plugins.

use bevy::prelude::*;

/// Tab API plugin placeholder
pub struct TabApiPlugin;

impl Plugin for TabApiPlugin {
    fn build(&self, _app: &mut App) {
        // TODO: Implement Tab API
    }
}

/// Tab registry resource
#[derive(Resource, Debug, Default)]
pub struct TabRegistry {
    pub tabs: Vec<PluginTab>,
}

impl TabRegistry {
    /// Get-or-insert by `tab.id` — NOT an unconditional push. More than one
    /// `StudioPlugin` legitimately wants to ensure the shared "plugins" tab
    /// exists (`RoadToolPlugin`, `PluginHostControlsPlugin`, any future
    /// script plugin's discovery) without knowing which of them runs
    /// first; an always-push here would silently create duplicate tab
    /// entries with the same id the moment a second plugin registered one.
    pub fn register_tab(&mut self, tab: PluginTab) {
        if self.tabs.iter().any(|t| t.id == tab.id) {
            return;
        }
        self.tabs.push(tab);
    }
    
    pub fn unregister_tab(&mut self, id: &str) {
        self.tabs.retain(|t| t.id != id);
    }
    
    pub fn get_all_tabs(&self) -> &[PluginTab] {
        &self.tabs
    }
    
    pub fn add_section(&mut self, tab_id: &str, section: TabSection) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            tab.sections.push(section);
        }
    }
    
    pub fn add_button(&mut self, tab_id: &str, section_name: &str, button: TabButton) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            if let Some(section) = tab.sections.iter_mut().find(|s| s.name == section_name) {
                section.buttons.push(button);
            }
        }
    }

    /// Remove one section (by id) from a tab — used to tear down a script
    /// plugin's own contributions on reload without touching the tab
    /// itself or any OTHER plugin's sections sharing it (e.g. the native
    /// Road Builder section on the same "plugins" tab).
    pub fn remove_section(&mut self, tab_id: &str, section_id: &str) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            tab.sections.retain(|s| s.id != section_id);
        }
    }

    /// Remove one button (by id) from a section — for a script plugin that
    /// adds buttons to an EXISTING section (its own or, in principle,
    /// another plugin's) rather than always creating a fresh section.
    pub fn remove_button(&mut self, tab_id: &str, section_id: &str, button_id: &str) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            if let Some(section) = tab.sections.iter_mut().find(|s| s.id == section_id) {
                section.buttons.retain(|b| b.id != button_id);
            }
        }
    }
}

/// Plugin tab definition
#[derive(Debug, Clone, Default)]
pub struct PluginTab {
    pub id: String,
    pub label: String,
    pub sections: Vec<TabSection>,
    pub icon: Option<String>,
    pub priority: i32,
    pub visible: bool,
    pub owner_plugin: Option<String>,
}

/// Tab section
#[derive(Debug, Clone, Default)]
pub struct TabSection {
    pub name: String,
    pub buttons: Vec<TabButton>,
    pub id: String,
    pub label: String,
    pub collapsible: bool,
    pub collapsed: bool,
}

/// Tab button
#[derive(Debug, Clone, Default)]
pub struct TabButton {
    pub label: String,
    pub icon: Option<String>,
    pub action: String,
    pub size: TabButtonSize,
    pub id: String,
    pub tooltip: Option<String>,
    pub action_id: String,
}

/// Tab button size
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabButtonSize {
    #[default]
    Small,
    Medium,
    Large,
    Normal,
}

/// Dropdown item
#[derive(Debug, Clone, Default)]
pub struct DropdownItem {
    pub label: String,
    pub value: String,
}

/// Tab API trait
pub trait TabApi {
    fn register_tab(&mut self, tab: PluginTab);
    fn unregister_tab(&mut self, id: &str);
}

impl TabApi for TabRegistry {
    fn register_tab(&mut self, tab: PluginTab) {
        // Delegates to the inherent method — see its doc comment for why
        // this must be get-or-insert, not an unconditional push.
        TabRegistry::register_tab(self, tab);
    }

    fn unregister_tab(&mut self, id: &str) {
        self.tabs.retain(|t| t.id != id);
    }
}

/// Custom tab modal
#[derive(Debug, Clone, Default)]
pub struct CustomTabModal {
    pub title: String,
    pub content: String,
    pub visible: bool,
}
