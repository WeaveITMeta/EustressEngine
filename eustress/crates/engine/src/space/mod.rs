/// Space management system - file-system-first architecture
/// 
/// A Space is a self-contained simulation environment:
/// - One Space = One scene = One folder
/// - Player-named (e.g., "My RPG", "City Builder", "Space Station")
/// - Git-native with sparse checkout for packages
/// - Can teleport between Spaces (scene transitions)
/// - Can load remote Spaces from .pak files (Cloudflare R2)

use bevy::prelude::*;
use std::path::PathBuf;

pub mod file_loader;
pub mod file_watcher;
pub mod instance_loader;

/// Resource holding the current Space root path
#[derive(Resource, Debug, Clone)]
pub struct SpaceRoot(pub PathBuf);

impl Default for SpaceRoot {
    fn default() -> Self {
        Self(default_space_root())
    }
}

/// Resolve the default Space root directory.
/// Priority: Documents/Eustress/Universe1/spaces/Space1 → Documents/Eustress/Universe1 → Documents/Eustress → current dir
pub fn default_space_root() -> PathBuf {
    if let Some(docs) = dirs::document_dir() {
        // Check for Documents/Eustress/Universe1/spaces/Space1 (default Space)
        let space1 = docs.join("Eustress").join("Universe1").join("spaces").join("Space1");
        if space1.exists() && space1.is_dir() {
            return space1;
        }
        
        // Check for Documents/Eustress/Universe1 (Universe root)
        let universe1 = docs.join("Eustress").join("Universe1");
        if universe1.exists() && universe1.is_dir() {
            return universe1;
        }
        
        // Check for Documents/Eustress (workspace root)
        let eustress = docs.join("Eustress");
        if eustress.exists() && eustress.is_dir() {
            return eustress;
        }
        
        return docs;
    }
    
    // Fallback to current directory
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub use file_loader::{
    FileType, FileMetadata, SpaceFileRegistry, LoadedFromFile,
    SpaceFileLoaderPlugin, scan_space_directory,
};
pub use file_watcher::{
    SpaceFileWatcher, FileChangeEvent, FileChangeType,
};
pub use instance_loader::{
    InstanceDefinition, InstanceFile, AssetReference,
    TransformData, InstanceProperties, InstanceMetadata,
    load_instance_definition, write_instance_definition,
};

