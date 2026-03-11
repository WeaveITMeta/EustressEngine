//! # Rune UI Bindings
//!
//! Exposes UI element read/write operations to Rune scripts.
//! Generalized system for any scriptable UI, not hardcoded to specific use cases.
//!
//! ## Table of Contents
//!
//! 1. **UIBindings** — Resource holding UI element state accessible from scripts
//! 2. **RuneUIModule** — Rune module registration for ui:: namespace
//! 3. **Systems** — Sync ECS ↔ UIBindings, execute scripts

use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::runtime_ui::{GuiElement, RuntimeUIManager};

/// Shared UI state accessible from Rune scripts
/// Uses Arc<RwLock<>> for thread-safe script access
#[derive(Resource, Clone)]
pub struct UIBindings {
    /// Element properties by path (e.g., "BatteryHUD/VoltageLabel")
    pub elements: Arc<RwLock<HashMap<String, UIElementState>>>,
    
    /// Pending property updates from scripts (path → property → value)
    pub pending_updates: Arc<RwLock<Vec<UIPropertyUpdate>>>,
}

impl Default for UIBindings {
    fn default() -> Self {
        Self {
            elements: Arc::new(RwLock::new(HashMap::new())),
            pending_updates: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

/// State of a single UI element accessible from scripts
#[derive(Debug, Clone, Default)]
pub struct UIElementState {
    pub text: String,
    pub visible: bool,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub background_color: [f32; 4],
    pub text_color: [f32; 4],
    pub font_size: f32,
    pub z_index: i32,
}

/// A pending property update from a script
#[derive(Debug, Clone)]
pub struct UIPropertyUpdate {
    pub element_path: String,
    pub property: String,
    pub value: UIPropertyValue,
}

/// Property value types that can be set from scripts
#[derive(Debug, Clone)]
pub enum UIPropertyValue {
    Text(String),
    Bool(bool),
    Float(f32),
    Int(i32),
    Vec2([f32; 2]),
    Color([f32; 4]),
}

impl UIBindings {
    /// Get text of an element
    pub fn get_text(&self, path: &str) -> String {
        self.elements.read()
            .map(|e| e.get(path).map(|s| s.text.clone()).unwrap_or_default())
            .unwrap_or_default()
    }
    
    /// Set text of an element (queues update)
    pub fn set_text(&self, path: &str, text: &str) {
        if let Ok(mut updates) = self.pending_updates.write() {
            updates.push(UIPropertyUpdate {
                element_path: path.to_string(),
                property: "text".to_string(),
                value: UIPropertyValue::Text(text.to_string()),
            });
        }
    }
    
    /// Get visibility of an element
    pub fn get_visible(&self, path: &str) -> bool {
        self.elements.read()
            .map(|e| e.get(path).map(|s| s.visible).unwrap_or(true))
            .unwrap_or(true)
    }
    
    /// Set visibility of an element
    pub fn set_visible(&self, path: &str, visible: bool) {
        if let Ok(mut updates) = self.pending_updates.write() {
            updates.push(UIPropertyUpdate {
                element_path: path.to_string(),
                property: "visible".to_string(),
                value: UIPropertyValue::Bool(visible),
            });
        }
    }
    
    /// Set background color of an element
    pub fn set_background_color(&self, path: &str, r: f32, g: f32, b: f32, a: f32) {
        if let Ok(mut updates) = self.pending_updates.write() {
            updates.push(UIPropertyUpdate {
                element_path: path.to_string(),
                property: "background_color".to_string(),
                value: UIPropertyValue::Color([r, g, b, a]),
            });
        }
    }
    
    /// Set text color of an element
    pub fn set_text_color(&self, path: &str, r: f32, g: f32, b: f32, a: f32) {
        if let Ok(mut updates) = self.pending_updates.write() {
            updates.push(UIPropertyUpdate {
                element_path: path.to_string(),
                property: "text_color".to_string(),
                value: UIPropertyValue::Color([r, g, b, a]),
            });
        }
    }
    
    /// Set size of an element
    pub fn set_size(&self, path: &str, width: f32, height: f32) {
        if let Ok(mut updates) = self.pending_updates.write() {
            updates.push(UIPropertyUpdate {
                element_path: path.to_string(),
                property: "size".to_string(),
                value: UIPropertyValue::Vec2([width, height]),
            });
        }
    }
    
    /// Set position of an element
    pub fn set_position(&self, path: &str, x: f32, y: f32) {
        if let Ok(mut updates) = self.pending_updates.write() {
            updates.push(UIPropertyUpdate {
                element_path: path.to_string(),
                property: "position".to_string(),
                value: UIPropertyValue::Vec2([x, y]),
            });
        }
    }
    
    /// Drain pending updates (called by sync system)
    pub fn drain_updates(&self) -> Vec<UIPropertyUpdate> {
        self.pending_updates.write()
            .map(|mut u| std::mem::take(&mut *u))
            .unwrap_or_default()
    }
    
    /// Update element state from GuiElement (called by sync system)
    pub fn sync_element(&self, path: &str, element: &GuiElement) {
        if let Ok(mut elements) = self.elements.write() {
            elements.insert(path.to_string(), UIElementState {
                text: element.text.clone().unwrap_or_default(),
                visible: element.visible,
                position: element.position,
                size: element.size,
                background_color: element.background_color,
                text_color: element.text_color.unwrap_or([1.0, 1.0, 1.0, 1.0]),
                font_size: element.font_size.unwrap_or(14.0),
                z_index: element.z_index,
            });
        }
    }
}

/// Plugin for Rune UI bindings
pub struct RuneUIBindingsPlugin;

impl Plugin for RuneUIBindingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UIBindings>()
            .add_systems(Update, (
                sync_ui_to_bindings,
                apply_script_updates,
            ).chain());
    }
}

/// Sync RuntimeUIManager elements to UIBindings for script access
fn sync_ui_to_bindings(
    ui_manager: Res<RuntimeUIManager>,
    bindings: Res<UIBindings>,
) {
    for (_entity, screen_gui) in &ui_manager.screen_guis {
        sync_element_tree(&bindings, "", &screen_gui.elements);
    }
}

fn sync_element_tree(bindings: &UIBindings, parent_path: &str, elements: &HashMap<String, GuiElement>) {
    for (name, element) in elements {
        let path = if parent_path.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", parent_path, name)
        };
        
        bindings.sync_element(&path, element);
        
        for child in &element.children {
            let child_map: HashMap<String, GuiElement> = 
                std::iter::once((child.name.clone(), child.clone())).collect();
            sync_element_tree(bindings, &path, &child_map);
        }
    }
}

/// Apply pending script updates to RuntimeUIManager
fn apply_script_updates(
    bindings: Res<UIBindings>,
    mut ui_manager: ResMut<RuntimeUIManager>,
) {
    let updates = bindings.drain_updates();
    
    for update in updates {
        for (_entity, screen_gui) in ui_manager.screen_guis.iter_mut() {
            if let Some(element) = find_element_mut(&mut screen_gui.elements, &update.element_path) {
                apply_update(element, &update);
            }
        }
    }
}

fn find_element_mut<'a>(
    elements: &'a mut HashMap<String, GuiElement>,
    path: &str,
) -> Option<&'a mut GuiElement> {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.is_empty() {
        return None;
    }
    
    let first = parts[0];
    let element = elements.get_mut(first)?;
    
    if parts.len() == 1 {
        return Some(element);
    }
    
    let remaining = parts[1..].join("/");
    find_element_in_children_mut(element, &remaining)
}

fn find_element_in_children_mut<'a>(
    parent: &'a mut GuiElement,
    path: &str,
) -> Option<&'a mut GuiElement> {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.is_empty() {
        return None;
    }
    
    let first = parts[0];
    
    for child in &mut parent.children {
        if child.name == first {
            if parts.len() == 1 {
                return Some(child);
            }
            let remaining = parts[1..].join("/");
            return find_element_in_children_mut(child, &remaining);
        }
    }
    
    None
}

fn apply_update(element: &mut GuiElement, update: &UIPropertyUpdate) {
    match (&update.property[..], &update.value) {
        ("text", UIPropertyValue::Text(t)) => element.text = Some(t.clone()),
        ("visible", UIPropertyValue::Bool(v)) => element.visible = *v,
        ("size", UIPropertyValue::Vec2(s)) => element.size = *s,
        ("position", UIPropertyValue::Vec2(p)) => element.position = *p,
        ("background_color", UIPropertyValue::Color(c)) => element.background_color = *c,
        ("text_color", UIPropertyValue::Color(c)) => element.text_color = Some(*c),
        _ => {}
    }
}
