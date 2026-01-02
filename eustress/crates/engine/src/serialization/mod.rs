// Serialization Module - Supports multiple formats:
// - Binary (.eustressengine) - Primary format, scales to millions of instances
// - JSON (legacy) - Human-readable, for debugging
// - RON (unified v3) - Structured text format

pub mod scene;
pub mod binary;

#[allow(unused_imports)]
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

/// Load a unified scene from RON file
pub fn load_unified_scene(path: &std::path::Path) -> Result<unified::Scene> {
    let content = std::fs::read_to_string(path)?;
    let scene: unified::Scene = ron::from_str(&content)?;
    Ok(scene)
}

/// Save a unified scene to RON file
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
