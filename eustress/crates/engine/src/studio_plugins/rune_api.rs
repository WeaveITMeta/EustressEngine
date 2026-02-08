//! # Rune API (Studio Plugins)
//!
//! Stub module for Rune API in studio plugins.

use bevy::prelude::*;

/// Rune API plugin placeholder
pub struct RuneApiPlugin;

impl Plugin for RuneApiPlugin {
    fn build(&self, _app: &mut App) {
        // TODO: Implement Rune API
    }
}

/// Rune execution context
#[derive(Debug, Clone, Default)]
pub struct RuneContext;

/// Rune value type
#[derive(Debug, Clone)]
pub enum RuneValue {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
}

impl Default for RuneValue {
    fn default() -> Self {
        RuneValue::Nil
    }
}

/// Billboard options
#[derive(Debug, Clone, Default)]
pub struct BillboardOptions {
    pub size: f32,
    pub always_on_top: bool,
}

/// Billboard request
#[derive(Debug, Clone, Default)]
pub struct BillboardRequest {
    pub text: String,
    pub options: BillboardOptions,
}

/// Screen UI element
#[derive(Debug, Clone)]
pub enum ScreenUIElement {
    Button(ScreenButton),
    Slider(ScreenSlider),
}

/// Screen button
#[derive(Debug, Clone, Default)]
pub struct ScreenButton {
    pub text: String,
    pub callback: String,
}

/// Screen slider
#[derive(Debug, Clone, Default)]
pub struct ScreenSlider {
    pub min: f32,
    pub max: f32,
    pub value: f32,
}

/// Hot reload watcher
#[derive(Resource, Debug, Default)]
pub struct HotReloadWatcher {
    pub watching: bool,
}

/// Menu registration
#[derive(Debug, Clone, Default)]
pub struct MenuRegistration {
    pub items: Vec<MenuItem>,
}

/// Menu item
#[derive(Debug, Clone, Default)]
pub struct MenuItem {
    pub label: String,
    pub action: String,
}

/// Tab registration
#[derive(Debug, Clone, Default)]
pub struct TabRegistration {
    pub sections: Vec<TabSectionDef>,
}

/// Tab section definition
#[derive(Debug, Clone, Default)]
pub struct TabSectionDef {
    pub name: String,
    pub buttons: Vec<TabButtonDef>,
}

/// Tab button definition
#[derive(Debug, Clone, Default)]
pub struct TabButtonDef {
    pub label: String,
    pub action: String,
}

/// Panel request
#[derive(Debug, Clone, Default)]
pub struct PanelRequest {
    pub title: String,
    pub content: String,
}

/// Screen GUI request
#[derive(Debug, Clone, Default)]
pub struct ScreenGuiRequest {
    pub elements: Vec<ScreenGuiElementDef>,
}

/// Screen GUI element definition
#[derive(Debug, Clone)]
pub struct ScreenGuiElementDef {
    pub element_type: String,
    pub properties: std::collections::HashMap<String, String>,
}

impl Default for ScreenGuiElementDef {
    fn default() -> Self {
        Self {
            element_type: String::new(),
            properties: std::collections::HashMap::new(),
        }
    }
}
