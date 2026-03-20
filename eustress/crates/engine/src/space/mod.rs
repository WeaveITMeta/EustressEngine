/// Space management system - file-system-first architecture
/// 
/// A Space is a self-contained simulation environment:
/// - One Space = One scene = One folder
/// - Player-named (e.g., "My RPG", "City Builder", "Space Station")
/// - Git-native with sparse checkout for packages
/// - Can teleport between Spaces (scene transitions)
/// - Can load remote Spaces from .pak files (Cloudflare R2)

use bevy::prelude::*;
use std::path::{Path, PathBuf};

pub mod class_defaults;
pub mod file_loader;
pub mod file_watcher;
pub mod gui_loader;
pub mod instance_loader;
pub mod material_loader;
pub mod service_loader;
pub mod draco_decoder;
pub mod space_ops;

/// Resource holding the current Space root path
#[derive(Resource, Debug, Clone)]
pub struct SpaceRoot(pub PathBuf);

impl Default for SpaceRoot {
    fn default() -> Self {
        Self(default_space_root())
    }
}

pub fn workspace_root() -> PathBuf {
    if let Some(docs) = dirs::document_dir() {
        let root = docs.join("Eustress");
        let _ = std::fs::create_dir_all(&root);
        return root;
    }

    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn looks_like_space_root(path: &Path) -> bool {
    path.join(".eustress").join("project.toml").exists()
        || path.join("Workspace").exists()
        || path.join("space.toml").exists()
}

pub fn universe_root_for_path(path: &Path) -> Option<PathBuf> {
    let workspace = workspace_root();

    if path.parent() == Some(workspace.as_path()) && !looks_like_space_root(path) {
        return Some(path.to_path_buf());
    }

    let parent = path.parent()?;
    if parent.parent() == Some(workspace.as_path()) {
        return Some(parent.to_path_buf());
    }

    None
}

pub fn first_universe_root() -> Option<PathBuf> {
    let workspace = workspace_root();
    let mut universes: Vec<PathBuf> = std::fs::read_dir(&workspace)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && !looks_like_space_root(path))
        .collect();
    universes.sort();
    universes.into_iter().next()
}

pub fn first_space_root_in_universe(universe_root: &Path) -> Option<PathBuf> {
    // First check the "spaces/" subdirectory (standard Universe structure)
    let spaces_dir = universe_root.join("spaces");
    if spaces_dir.is_dir() {
        let mut spaces: Vec<PathBuf> = std::fs::read_dir(&spaces_dir)
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir() && looks_like_space_root(path))
            .collect();
        spaces.sort();
        if let Some(space) = spaces.into_iter().next() {
            return Some(space);
        }
    }
    
    // Fallback: check directly inside the universe root (legacy structure)
    let mut spaces: Vec<PathBuf> = std::fs::read_dir(universe_root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && looks_like_space_root(path))
        .collect();
    spaces.sort();
    spaces.into_iter().next()
}

pub fn default_space_root() -> PathBuf {
    let workspace = workspace_root();

    if let Ok(read_dir) = std::fs::read_dir(&workspace) {
        let mut universes: Vec<PathBuf> = read_dir
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir() && !looks_like_space_root(path))
            .collect();
        universes.sort();

        for universe_root in universes {
            if let Some(space_root) = first_space_root_in_universe(&universe_root) {
                return space_root;
            }
        }
    }

    first_universe_root().unwrap_or(workspace)
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
    load_instance_definition, load_instance_definition_with_defaults,
    write_instance_definition,
};
pub use class_defaults::ClassDefaultsRegistry;

