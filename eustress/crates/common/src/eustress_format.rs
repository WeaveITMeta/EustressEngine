//! # Eustress File Format (.eustress / .eustressengine)
//!
//! ## DEPRECATION NOTICE
//!
//! The RON-based save/load functions in this module (`load_eustress`, `save_eustress`,
//! `save_for_engine`, `save_for_client`) are **deprecated**. The engine now uses:
//!
//! - **Binary format** for whole-scene save/load (see `serialization/binary.rs`)
//! - **TOML `.glb.toml`** for per-entity instance definitions (see `space/instance_loader.rs`)
//!
//! These RON functions are retained only for legacy file import and the `--scene` CLI flag.
//! New code should use `save_binary_scene` / `load_binary_scene_to_world` instead.
//!
//! ## File Extensions
//! - `.eustress` - Client/Player scene format (opens in Eustress Client)
//! - `.eustressengine` - Engine/Editor scene format (opens in Eustress Engine Studio)
//! - `.eproject` - Project manifest (references multiple scene files)
//!
//! ## Extension Philosophy
//! - `.eustress` = Published/Playable content → Opens in CLIENT
//! - `.eustressengine` = Development/Editable content → Opens in ENGINE
//!
//! The `.eustressengine` extension is now used by the binary format (magic bytes `EUSTRESS`).
//! Legacy RON files with this extension are detected by the absence of magic bytes.

use crate::scene::Scene;
use std::path::Path;
use std::io::Write;

// ============================================================================
// Constants
// ============================================================================

/// Unified scene extension — used by both Studio and Client.
/// Binary files start with `EUSTRESS` magic bytes (8 bytes).
pub const EXTENSION: &str = "eustress";

/// Project manifest extension
pub const EXTENSION_PROJECT: &str = "eproject";

/// All valid Eustress extensions
pub const VALID_EXTENSIONS: &[&str] = &["eustress", "eustressengine", "eproject"];

/// Legacy extensions (import only, will convert to .eustress on save)
/// Note: "eustressengine" is in VALID_EXTENSIONS for backward compat but NOT here,
/// because the RON load_eustress function rejects LEGACY_EXTENSIONS outright.
/// Old .eustressengine RON files are handled by the magic-byte detection in file_event_handler.
pub const LEGACY_EXTENSIONS: &[&str] = &["json", "ron", "escene", "rbxl", "rbxlx"];

/// Current format version
pub const FORMAT_VERSION: &str = "eustress_v5";

/// Magic bytes at start of binary format
pub const MAGIC_BYTES: &[u8; 4] = b"EUST";

/// Default extension for all saves
pub const DEFAULT_EXTENSION: &str = EXTENSION;

// ============================================================================
// Deprecated Aliases (will be removed)
// ============================================================================

/// DEPRECATED: Use `EXTENSION` instead. Client and Engine now share one extension.
#[deprecated(note = "Use EXTENSION instead. .eustressengine is deprecated in favor of .eustress")]
pub const EXTENSION_CLIENT: &str = "eustress";

/// DEPRECATED: Use `EXTENSION` instead. .eustressengine is replaced by .eustress.
#[deprecated(note = "Use EXTENSION instead. .eustressengine is deprecated in favor of .eustress")]
pub const EXTENSION_ENGINE: &str = "eustress";

/// DEPRECATED: Use `DEFAULT_EXTENSION` instead.
#[deprecated(note = "Use DEFAULT_EXTENSION instead")]
pub const DEFAULT_ENGINE_EXTENSION: &str = EXTENSION;

/// DEPRECATED: Use `DEFAULT_EXTENSION` instead.
#[deprecated(note = "Use DEFAULT_EXTENSION instead")]
pub const DEFAULT_CLIENT_EXTENSION: &str = EXTENSION;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur when working with .eustress files
#[derive(Debug)]
pub enum EustressError {
    /// File not found
    NotFound(String),
    /// IO error
    Io(std::io::Error),
    /// Parse error (invalid RON)
    Parse(String),
    /// Version mismatch
    VersionMismatch { expected: String, found: String },
    /// Invalid format
    InvalidFormat(String),
    /// Unsupported legacy format
    LegacyFormat(String),
}

impl std::fmt::Display for EustressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EustressError::NotFound(path) => write!(f, "File not found: {}", path),
            EustressError::Io(e) => write!(f, "IO error: {}", e),
            EustressError::Parse(e) => write!(f, "Parse error: {}", e),
            EustressError::VersionMismatch { expected, found } => {
                write!(f, "Version mismatch: expected {}, found {}", expected, found)
            }
            EustressError::InvalidFormat(e) => write!(f, "Invalid format: {}", e),
            EustressError::LegacyFormat(ext) => {
                write!(f, "Legacy format '{}' - import and save as .eustress", ext)
            }
        }
    }
}

impl std::error::Error for EustressError {}

impl From<std::io::Error> for EustressError {
    fn from(e: std::io::Error) -> Self {
        EustressError::Io(e)
    }
}

impl From<ron::error::SpannedError> for EustressError {
    fn from(e: ron::error::SpannedError) -> Self {
        EustressError::Parse(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, EustressError>;

// ============================================================================
// Core Functions
// ============================================================================

/// Load a .eustress or .eustressengine scene file (RON text format).
///
/// DEPRECATED: Use binary format (`serialization::load_binary_scene_to_world`) instead.
/// Retained for legacy file import and `--scene` CLI flag.
#[deprecated(note = "RON scene format is deprecated. Use binary format for new scenes.")]
pub fn load_eustress<P: AsRef<Path>>(path: P) -> Result<Scene> {
    let path = path.as_ref();
    
    // Check extension
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy().to_lowercase();
        
        // Check for legacy formats
        if LEGACY_EXTENSIONS.contains(&ext.as_str()) {
            return Err(EustressError::LegacyFormat(ext.to_string()));
        }
        
        // Verify it's a valid eustress extension
        if !VALID_EXTENSIONS.contains(&ext.as_str()) {
            return Err(EustressError::InvalidFormat(format!(
                "Expected .{} file, got .{}", 
                EXTENSION, ext
            )));
        }
    }
    
    // Check file exists
    if !path.exists() {
        return Err(EustressError::NotFound(path.display().to_string()));
    }
    
    // Read file
    let content = std::fs::read_to_string(path)?;
    
    // Parse RON
    let scene: Scene = ron::from_str(&content)?;
    
    // Verify format version (warn but don't fail for minor mismatches)
    if !scene.format.starts_with("eustress_") {
        return Err(EustressError::VersionMismatch {
            expected: FORMAT_VERSION.to_string(),
            found: scene.format.clone(),
        });
    }
    
    Ok(scene)
}

/// Save a scene to .eustress format (RON text).
/// If no valid extension is provided, defaults to .eustress.
///
/// DEPRECATED: Use binary format (`serialization::save_binary_scene`) instead.
#[deprecated(note = "RON scene format is deprecated. Use save_binary_scene instead.")]
pub fn save_eustress<P: AsRef<Path>>(scene: &Scene, path: P) -> Result<()> {
    let path = path.as_ref();
    
    // Check if extension is valid, otherwise default to .eustress
    let path = if let Some(ext) = path.extension() {
        let ext_str = ext.to_string_lossy().to_lowercase();
        if VALID_EXTENSIONS.contains(&ext_str.as_str()) {
            path.to_path_buf()
        } else {
            path.with_extension(DEFAULT_EXTENSION)
        }
    } else {
        path.with_extension(DEFAULT_EXTENSION)
    };
    
    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    
    // Serialize with pretty formatting
    let pretty = ron::ser::PrettyConfig::new()
        .depth_limit(10)
        .separate_tuple_members(true)
        .enumerate_arrays(false)
        .new_line("\n".to_string())
        .indentor("    ".to_string());
    
    let content = ron::ser::to_string_pretty(scene, pretty)
        .map_err(|e| EustressError::Parse(e.to_string()))?;
    
    // Determine file type for header
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let file_type = if ext == EXTENSION {
        "Eustress Scene"
    } else {
        "Eustress Scene (Legacy)"
    };
    
    // Add header comment
    let header = format!(
        "// Eustress {} - {}\n// Format: {}\n// Extension: .{}\n// DO NOT EDIT MANUALLY unless you know what you're doing\n\n",
        file_type,
        scene.metadata.name,
        FORMAT_VERSION,
        ext
    );
    
    // Write file
    let mut file = std::fs::File::create(&path)?;
    file.write_all(header.as_bytes())?;
    file.write_all(content.as_bytes())?;
    
    Ok(())
}

/// Save specifically for Engine (development) - uses .eustressengine (RON text).
///
/// DEPRECATED: Use binary format instead.
#[deprecated(note = "RON scene format is deprecated. Use save_binary_scene instead.")]
#[allow(deprecated)]
pub fn save_for_engine<P: AsRef<Path>>(scene: &Scene, path: P) -> Result<()> {
    let path = path.as_ref().with_extension(EXTENSION);
    save_eustress(scene, path)
}

/// Save specifically for Client (published) - uses .eustress (RON text).
///
/// DEPRECATED: Use binary format instead.
#[deprecated(note = "RON scene format is deprecated. Use save_binary_scene instead.")]
#[allow(deprecated)]
pub fn save_for_client<P: AsRef<Path>>(scene: &Scene, path: P) -> Result<()> {
    let path = path.as_ref().with_extension(EXTENSION);
    save_eustress(scene, path)
}

/// Check if a path is a valid Eustress file (client or engine)
pub fn is_eustress_file<P: AsRef<Path>>(path: P) -> bool {
    let path = path.as_ref();
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy().to_lowercase();
        VALID_EXTENSIONS.contains(&ext.as_str())
    } else {
        false
    }
}

/// Check if a path is a client scene (.eustress)
#[deprecated(note = "Use is_eustress_file instead. Client/Engine distinction removed.")]
pub fn is_client_scene<P: AsRef<Path>>(path: P) -> bool {
    let path = path.as_ref();
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase() == EXTENSION)
        .unwrap_or(false)
}

/// Check if a path is an engine scene (.eustressengine)
#[deprecated(note = "Use is_eustress_file instead. .eustressengine is now a legacy extension.")]
pub fn is_engine_scene<P: AsRef<Path>>(path: P) -> bool {
    let path = path.as_ref();
    path.extension()
        .map(|e| {
            let ext = e.to_string_lossy().to_lowercase();
            ext == EXTENSION || ext == "eustressengine"
        })
        .unwrap_or(false)
}

/// Check if a path is a legacy format that needs conversion
pub fn is_legacy_format<P: AsRef<Path>>(path: P) -> bool {
    let path = path.as_ref();
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy().to_lowercase();
        LEGACY_EXTENSIONS.contains(&ext.as_str())
    } else {
        false
    }
}

/// Convert a path to .eustress extension
pub fn to_eustress_path<P: AsRef<Path>>(path: P) -> std::path::PathBuf {
    path.as_ref().with_extension(EXTENSION)
}

/// DEPRECATED: Use to_eustress_path instead.
#[deprecated(note = "Use to_eustress_path instead. .eustressengine is deprecated.")]
pub fn to_engine_path<P: AsRef<Path>>(path: P) -> std::path::PathBuf {
    to_eustress_path(path)
}

/// DEPRECATED: Use to_eustress_path instead.
#[deprecated(note = "Use to_eustress_path instead. Client/Engine distinction removed.")]
pub fn to_client_path<P: AsRef<Path>>(path: P) -> std::path::PathBuf {
    to_eustress_path(path)
}

// ============================================================================
// Default Scene
// ============================================================================

/// Create a new default scene (empty place with baseplate)
pub fn new_default_scene(name: &str) -> Scene {
    use crate::scene::*;
    
    Scene {
        format: FORMAT_VERSION.to_string(),
        metadata: SceneMetadata {
            name: name.to_string(),
            author: whoami::username(),
            description: "A new Eustress scene".to_string(),
            created: chrono::Utc::now().to_rfc3339(),
            modified: chrono::Utc::now().to_rfc3339(),
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            tags: vec!["new".to_string()],
        },
        global_theme: "default".to_string(),
        atmosphere: AtmosphereSettings::default(),
        workspace_settings: WorkspaceSettings::default(),
        player_settings: PlayerSettings::default(),
        spawn_locations: vec![
            SpawnLocationData::default(),
        ],
        orbital_settings: OrbitalSettings::default(),
        entities: vec![
            // Baseplate
            Entity {
                id: 1,
                name: "Baseplate".to_string(),
                parent: None,
                children: vec![],
                class: EntityClass::Part(PartData {
                    size: [512.0, 1.0, 512.0],
                    color: [0.388, 0.372, 0.384, 1.0],
                    material: "Plastic".to_string(),
                    shape: "Block".to_string(),
                    transparency: 0.0,
                    reflectance: 0.0,
                    anchored: true,
                    can_collide: true,
                    cast_shadow: true,
                }),
                transform: TransformData {
                    position: [0.0, -0.5, 0.0],
                    rotation: [0.0, 0.0, 0.0, 1.0], // Quaternion: x, y, z, w
                    scale: [1.0, 1.0, 1.0],
                },
                network_ownership: NetworkOwnershipRule::ServerOnly,
                prompt: "large gray baseplate floor".to_string(),
                detail_level: DetailLevel::Low,
                category: NodeCategory::Structure,
                quest_flags: std::collections::HashMap::new(),
                generated_mesh_id: None,
                generated_texture_id: None,
                generated_lods: vec![],
                generation_status: GenerationStatus::NotRequested,
                archivable: true,
                ai: false,
            },
        ],
        connections: vec![],
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_is_eustress_file() {
        // Valid extensions
        assert!(is_eustress_file("game.eustress"));
        assert!(is_eustress_file("game.eustressengine"));
        assert!(is_eustress_file("game.eproject"));
        
        // Invalid/legacy
        assert!(!is_eustress_file("game.json"));
        assert!(!is_eustress_file("game.ron"));
        assert!(!is_eustress_file("game.escene")); // Now legacy
    }
    
    #[test]
    fn test_client_vs_engine() {
        assert!(is_client_scene("game.eustress"));
        assert!(!is_client_scene("game.eustressengine"));
        
        assert!(is_engine_scene("game.eustressengine"));
        assert!(!is_engine_scene("game.eustress"));
    }
    
    #[test]
    fn test_is_legacy_format() {
        assert!(is_legacy_format("game.json"));
        assert!(is_legacy_format("game.ron"));
        assert!(is_legacy_format("game.rbxl"));
        assert!(is_legacy_format("game.escene")); // Old extension is now legacy
        assert!(!is_legacy_format("game.eustress"));
        assert!(!is_legacy_format("game.eustressengine"));
    }
    
    #[test]
    fn test_path_conversions() {
        assert_eq!(
            to_engine_path("game.json"),
            std::path::PathBuf::from("game.eustressengine")
        );
        assert_eq!(
            to_client_path("game.json"),
            std::path::PathBuf::from("game.eustress")
        );
        // Legacy alias
        assert_eq!(
            to_eustress_path("game.json"),
            std::path::PathBuf::from("game.eustressengine")
        );
    }
}
