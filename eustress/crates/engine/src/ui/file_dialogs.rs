//! # Eustress File Dialogs
//!
//! All scenes use `.eustress` extension (binary format, `EUSTRESS` magic bytes).
//! Legacy formats (.eustressengine, .json, .ron, .escene) are supported for import only.

use bevy::prelude::*;
use std::path::PathBuf;
use eustress_common::{
    EXTENSION, EXTENSION_PROJECT,
    VALID_EXTENSIONS, LEGACY_EXTENSIONS,
};

/// Resource tracking current scene file
#[derive(Resource, Default, Clone)]
pub struct SceneFile {
    /// Path to the current scene file
    pub path: Option<PathBuf>,
    
    /// Whether the scene has unsaved changes
    pub modified: bool,
    
    /// Display name for the scene
    pub name: String,
}

impl SceneFile {
    /// Create a new untitled scene
    pub fn new_untitled() -> Self {
        Self {
            path: None,
            modified: false,
            name: "Untitled".to_string(),
        }
    }
    
    /// Create from a file path
    pub fn from_path(path: PathBuf) -> Self {
        let name = path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_string());
        
        Self {
            path: Some(path),
            modified: false,
            name,
        }
    }
    
    /// Get the window title
    pub fn window_title(&self) -> String {
        let dirty = if self.modified { "*" } else { "" };
        format!("{}{} - Eustress Engine", dirty, self.name)
    }
    
    /// Mark as modified
    pub fn mark_modified(&mut self) {
        self.modified = true;
    }
    
    /// Mark as saved
    pub fn mark_saved(&mut self) {
        self.modified = false;
    }
    
    /// Check if this is a legacy format that needs conversion
    pub fn is_legacy_format(&self) -> bool {
        if let Some(ref path) = self.path {
            if let Some(ext) = path.extension() {
                let ext = ext.to_string_lossy().to_lowercase();
                return LEGACY_EXTENSIONS.contains(&ext.as_str());
            }
        }
        false
    }
    
    /// Check if this is an Eustress scene (.eustress)
    pub fn is_eustress_scene(&self) -> bool {
        self.path.as_ref()
            .and_then(|p| p.extension())
            .map(|e| e.to_string_lossy().to_lowercase() == EXTENSION)
            .unwrap_or(false)
    }
    
    /// Get the path with .eustress extension
    pub fn path_as_eustress(&self) -> Option<PathBuf> {
        self.path.as_ref().map(|p| p.with_extension(EXTENSION))
    }
}

/// Events for file operations
#[derive(Message)]
pub enum FileEvent {
    /// Create a new empty scene
    NewScene,
    /// Open an existing scene file
    OpenScene,
    /// Save the current scene (or SaveAs if untitled)
    SaveScene,
    /// Save the current scene to a new file
    SaveSceneAs,
    /// Open a recent scene by path
    OpenRecent(PathBuf),
    /// Publish the experience to Eustress platform
    Publish,
    /// Publish with new name/settings
    PublishAs,
}

/// Show file picker for opening scenes
/// Prioritizes .eustress but accepts legacy formats for import
pub fn pick_open_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Eustress Scene", &["eustress"])
        .add_filter("Legacy Formats", &["eustressengine", "ron", "json", "escene"])
        .add_filter("All Scenes", &["eustress", "eustressengine", "ron", "json", "escene"])
        .set_title("Open Scene")
        .pick_file()
}

/// Show file picker for saving scenes (binary .eustress format)
pub fn pick_save_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Eustress Scene", &["eustress"])
        .set_title("Save Scene")
        .set_file_name("scene.eustress")
        .save_file()
}

/// Show file picker for publishing (same format, just explicit naming)
pub fn pick_publish_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Eustress Scene", &["eustress"])
        .set_title("Publish Scene")
        .set_file_name("scene.eustress")
        .save_file()
}

/// Get the default scenes directory
pub fn default_scenes_dir() -> PathBuf {
    // Try to use Documents/Eustress/Scenes
    if let Some(docs) = dirs::document_dir() {
        let scenes_dir = docs.join("Eustress").join("Scenes");
        if scenes_dir.exists() || std::fs::create_dir_all(&scenes_dir).is_ok() {
            return scenes_dir;
        }
    }
    
    // Fallback to current directory
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Get the default scene file path for new projects
pub fn default_scene_path() -> PathBuf {
    default_scenes_dir().join(format!("Untitled.{}", EXTENSION))
}
