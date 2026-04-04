#![allow(dead_code)]

use serde::{Serialize, Deserialize};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Global selection state with multi-select support
pub struct SelectionManager {
    selected: Mutex<Vec<String>>,
    clipboard: Mutex<Vec<String>>,
    /// Monotonically increasing counter — bumped on every selection mutation.
    /// Allows systems to skip work when nothing changed.
    generation: AtomicU64,
}

impl Default for SelectionManager {
    fn default() -> Self {
        Self {
            selected: Mutex::new(Vec::new()),
            clipboard: Mutex::new(Vec::new()),
            generation: AtomicU64::new(0),
        }
    }
}

impl SelectionManager {
    /// Current generation counter (changes on every selection mutation).
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Select a single entity (clears previous selection)
    pub fn select(&self, id: String) {
        let mut selected = self.selected.lock().expect("SelectionManager mutex poisoned");
        selected.clear();
        selected.push(id);
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Add entity to selection (multi-select)
    pub fn add_to_selection(&self, id: String) {
        let mut selected = self.selected.lock().expect("SelectionManager mutex poisoned");
        if !selected.contains(&id) {
            selected.push(id);
            self.generation.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Remove entity from selection
    pub fn remove_from_selection(&self, id: &str) {
        let mut selected = self.selected.lock().expect("SelectionManager mutex poisoned");
        let before = selected.len();
        selected.retain(|s| s != id);
        if selected.len() != before {
            self.generation.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Toggle entity selection
    pub fn toggle_selection(&self, id: String) {
        let mut selected = self.selected.lock().expect("SelectionManager mutex poisoned");
        if let Some(pos) = selected.iter().position(|s| s == &id) {
            selected.remove(pos);
        } else {
            selected.push(id);
        }
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Get all selected entities
    pub fn get_selected(&self) -> Vec<String> {
        self.selected.lock().expect("SelectionManager mutex poisoned").clone()
    }

    /// Check if an entity is selected
    pub fn is_selected(&self, id: &str) -> bool {
        self.selected.lock().expect("SelectionManager mutex poisoned").contains(&id.to_string())
    }

    /// Get selection count
    pub fn selection_count(&self) -> usize {
        self.selected.lock().expect("SelectionManager mutex poisoned").len()
    }

    /// Clear all selections
    pub fn clear(&self) {
        let mut selected = self.selected.lock().expect("SelectionManager mutex poisoned");
        if !selected.is_empty() {
            selected.clear();
            self.generation.fetch_add(1, Ordering::Relaxed);
        }
    }
    
    /// Copy selected to clipboard
    pub fn copy_to_clipboard(&self) {
        let selected = self.selected.lock().expect("SelectionManager mutex poisoned").clone();
        *self.clipboard.lock().expect("SelectionManager clipboard mutex poisoned") = selected;
    }
    
    /// Get clipboard contents
    pub fn get_clipboard(&self) -> Vec<String> {
        self.clipboard.lock().expect("SelectionManager clipboard mutex poisoned").clone()
    }
    
    /// Check if clipboard has content
    pub fn has_clipboard_content(&self) -> bool {
        !self.clipboard.lock().expect("SelectionManager clipboard mutex poisoned").is_empty()
    }
}

/// Transform operation mode
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransformMode {
    Local,
    Global,
}

/// Global transform state
#[derive(Default)]
pub struct TransformManager {
    mode: Mutex<TransformMode>,
    snap_enabled: Mutex<bool>,
    snap_increment: Mutex<f32>,
}

impl Default for TransformMode {
    fn default() -> Self {
        TransformMode::Local
    }
}

impl TransformManager {
    pub fn set_mode(&self, mode: TransformMode) {
        *self.mode.lock().expect("TransformManager mode mutex poisoned") = mode;
    }
    
    pub fn get_mode(&self) -> TransformMode {
        self.mode.lock().expect("TransformManager mode mutex poisoned").clone()
    }
    
    pub fn toggle_snap(&self) {
        let mut snap = self.snap_enabled.lock().expect("TransformManager snap mutex poisoned");
        *snap = !*snap;
    }
    
    pub fn get_snap_settings(&self) -> (bool, f32) {
        let enabled = *self.snap_enabled.lock().expect("TransformManager snap mutex poisoned");
        let increment = *self.snap_increment.lock().expect("TransformManager snap_increment mutex poisoned");
        (enabled, increment)
    }
}
