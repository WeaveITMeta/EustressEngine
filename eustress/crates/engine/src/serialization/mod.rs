// Serialization Module
//
// PRIMARY FORMAT:
//   Binary (.eustress) — high-performance, scales to millions of instances.
//   Used by: Save/Open (UI), auto-save, play-mode snapshot.
//
// DEPRECATED FORMATS (import-only, will be removed):
//   JSON (.scene.json) — legacy PropertyAccess scene format from Tauri era.
//   RON (unified v3) — legacy structured text, only used by --scene CLI flag.
//
// FILE-SYSTEM-FIRST FORMAT (per-entity, not whole-scene):
//   TOML (.glb.toml) — instance definitions. See space/instance_loader.rs.

/// DEPRECATED: JSON PropertyAccess scene format. Use binary format instead.
pub mod scene;
pub mod binary;

#[allow(unused_imports)]
#[deprecated(note = "JSON scene format is deprecated. Use save_binary_scene/load_binary_scene_to_world instead.")]
pub use scene::{save_scene, load_scene, load_scene_from_world, Scene, EntityData, SceneMetadata};

// Binary format for high-performance serialization (millions of instances)
pub use binary::{
    save_binary_scene, load_binary_scene, load_binary_scene_to_world,
    BinaryEntityData, FileHeader, StringTable,
    ClassId, BinaryError,
};

// Re-export unified scene format from common
pub use eustress_common::scene as unified;

/// Error type for serialization operations
#[derive(Debug)]
pub enum SerializationError {
    IoError(std::io::Error),
    JsonError(serde_json::Error),
    RonError(String),
    InvalidFormat(String),
    MissingProperty(String),
    InvalidClass(String),
}

impl std::fmt::Display for SerializationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SerializationError::IoError(e) => write!(f, "IO Error: {}", e),
            SerializationError::JsonError(e) => write!(f, "JSON Error: {}", e),
            SerializationError::RonError(e) => write!(f, "RON Error: {}", e),
            SerializationError::InvalidFormat(s) => write!(f, "Invalid Format: {}", s),
            SerializationError::MissingProperty(s) => write!(f, "Missing Property: {}", s),
            SerializationError::InvalidClass(s) => write!(f, "Invalid Class: {}", s),
        }
    }
}

impl std::error::Error for SerializationError {}

impl From<std::io::Error> for SerializationError {
    fn from(e: std::io::Error) -> Self {
        SerializationError::IoError(e)
    }
}

impl From<serde_json::Error> for SerializationError {
    fn from(e: serde_json::Error) -> Self {
        SerializationError::JsonError(e)
    }
}

impl From<ron::error::SpannedError> for SerializationError {
    fn from(e: ron::error::SpannedError) -> Self {
        SerializationError::RonError(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, SerializationError>;

/// Load a unified scene from RON file.
///
/// DEPRECATED: RON scene format is legacy. New scenes use binary format.
/// This function is retained for --scene CLI flag and legacy file import.
#[deprecated(note = "RON scene format is deprecated. Use binary format for new scenes.")]
pub fn load_unified_scene(path: &std::path::Path) -> Result<unified::Scene> {
    let content = std::fs::read_to_string(path)?;
    let scene: unified::Scene = ron::from_str(&content)?;
    Ok(scene)
}

/// Save a unified scene to RON file.
///
/// DEPRECATED: RON scene format is legacy. New scenes use binary format.
#[deprecated(note = "RON scene format is deprecated. Use save_binary_scene instead.")]
pub fn save_unified_scene(scene: &unified::Scene, path: &std::path::Path) -> Result<()> {
    let pretty = ron::ser::PrettyConfig::new()
        .depth_limit(8)
        .separate_tuple_members(true)
        .enumerate_arrays(false);
    
    let content = ron::ser::to_string_pretty(scene, pretty)
        .map_err(|e| SerializationError::RonError(e.to_string()))?;
    
    std::fs::write(path, content)?;
    Ok(())
}
