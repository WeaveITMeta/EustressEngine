#![allow(dead_code)]

use serde::{Serialize, Deserialize};
use std::sync::Mutex;

/// Scene metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneMetadata {
    pub name: String,
    pub description: Option<String>,
    pub created: String,
    pub modified: String,
}

/// Global scene management state
#[derive(Default)]
pub struct SceneManagerState {
    current_scene: Mutex<Option<String>>,
    metadata: Mutex<Option<SceneMetadata>>,
}

impl SceneManagerState {
    pub fn get_current_scene(&self) -> Option<String> {
        self.current_scene.lock().unwrap().clone()
    }
    
    pub fn set_current_scene(&self, name: Option<String>) {
        *self.current_scene.lock().unwrap() = name;
    }
}
