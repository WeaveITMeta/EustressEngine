/// Space management system - file-system-first architecture
/// 
/// A Space is a self-contained simulation environment:
/// - One Space = One scene = One folder
/// - Player-named (e.g., "My RPG", "City Builder", "Space Station")
/// - Git-native with sparse checkout for packages
/// - Can teleport between Spaces (scene transitions)
/// - Can load remote Spaces from .pak files (Cloudflare R2)

pub mod file_loader;
pub mod file_watcher;
pub mod instance_loader;

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
