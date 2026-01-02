//! Common type definitions

use serde::{Deserialize, Serialize};

/// Asset handle wrapper for serialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetPath(pub String);

/// Material definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialDef {
    pub name: String,
    pub color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
}

impl Default for MaterialDef {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            color: [0.8, 0.8, 0.8, 1.0],
            metallic: 0.0,
            roughness: 0.5,
        }
    }
}
