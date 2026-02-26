/// Dynamic file loader - scans Space folders and automatically loads supported file types
/// 
/// This system replaces hardcoded entity spawning with automatic discovery:
/// - Scans Workspace/, Lighting/, etc. folders for supported files
/// - Loads .glb files as meshes, .soul files as scripts, .png as textures, etc.
/// - Creates ECS entities dynamically based on file contents
/// - Watches for file changes and reloads automatically
/// - Properties panel edits actual files on disk, not just in-memory ECS

use bevy::prelude::*;
use std::path::{Path, PathBuf};
use std::collections::HashMap;

/// Supported file types and their loaders
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileType {
    // 3D Models
    Gltf,           // .gltf, .glb
    Obj,            // .obj
    Fbx,            // .fbx
    
    // Scripts
    Soul,           // .soul → .md (markdown scripts that compile to .rune in cache)
    Rune,           // .rune (compiled Rune bytecode in cache)
    Wasm,           // .wasm (compiled scripts)
    Lua,            // .lua
    
    // Textures
    Png,            // .png
    Jpg,            // .jpg, .jpeg
    Tga,            // .tga
    Dds,            // .dds
    Ktx2,           // .ktx2
    
    // Audio
    Ogg,            // .ogg
    Mp3,            // .mp3
    Wav,            // .wav
    Flac,           // .flac
    
    // Scenes
    Scene,          // .scene.toml
    
    // Materials
    Material,       // .mat.toml
    
    // Terrain
    Hgt,            // .hgt (SRTM elevation)
    GeoTiff,        // .tif, .tiff
    
    // UI
    Slint,          // .slint
    Html,           // .html
    
    // Data
    Json,           // .json
    Toml,           // .toml
    Ron,            // .ron

    // Virtual — represents a filesystem subdirectory mapped to a Folder entity
    Directory,
}

impl FileType {
    /// Get file type from extension
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "gltf" | "glb" => Some(Self::Gltf),
            "obj" => Some(Self::Obj),
            "fbx" => Some(Self::Fbx),
            "soul" | "md" => Some(Self::Soul), // .soul or .md files compile to .rune
            "rune" => Some(Self::Rune), // Compiled bytecode in cache
            "wasm" => Some(Self::Wasm),
            "lua" => Some(Self::Lua),
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpg),
            "tga" => Some(Self::Tga),
            "dds" => Some(Self::Dds),
            "ktx2" => Some(Self::Ktx2),
            "ogg" => Some(Self::Ogg),
            "mp3" => Some(Self::Mp3),
            "wav" => Some(Self::Wav),
            "flac" => Some(Self::Flac),
            "hgt" => Some(Self::Hgt),
            "tif" | "tiff" | "geotiff" => Some(Self::GeoTiff),
            "slint" => Some(Self::Slint),
            "html" => Some(Self::Html),
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "ron" => Some(Self::Ron),
            _ => None,
        }
    }
    
    /// Get file type from full path (handles compound extensions like .glb.toml)
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        let path_str = path.to_string_lossy();
        
        // Check for compound extensions first
        if path_str.ends_with(".glb.toml") {
            return Some(Self::Toml); // Instance file
        } else if path_str.ends_with(".scene.toml") {
            return Some(Self::Scene);
        } else if path_str.ends_with(".mat.toml") {
            return Some(Self::Material);
        }
        
        // Fall back to simple extension check
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(Self::from_extension)
    }
    
    /// Check if this file type should spawn an entity in the given service folder
    pub fn spawns_entity_in_service(&self, service: &str) -> bool {
        match (self, service) {
            // Workspace: Instance files (.glb.toml) and 3D models spawn as Parts
            (Self::Toml | Self::Gltf | Self::Obj | Self::Fbx, "Workspace") => true,
            
            // Lighting: Models can be light sources
            (Self::Gltf | Self::Obj | Self::Fbx, "Lighting") => true,
            
            // SoulService: Scripts don't spawn visible entities, but need to be loaded
            (Self::Soul | Self::Rune | Self::Wasm | Self::Lua, "SoulService") => true,
            
            // SoundService: Audio files spawn as Sound entities
            (Self::Ogg | Self::Mp3 | Self::Wav | Self::Flac, "SoundService") => true,
            
            // StarterGui: UI files don't spawn in 3D world
            (Self::Slint | Self::Html, "StarterGui") => false,
            
            // Default: don't spawn
            _ => false,
        }
    }
}

/// Metadata extracted from a file or directory
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub path: PathBuf,
    pub file_type: FileType,
    pub service: String,
    pub name: String,
    pub size: u64,
    pub modified: std::time::SystemTime,
    /// For Directory entries: the child file entries inside this directory
    pub children: Vec<FileMetadata>,
}

/// Resource tracking all loaded files in the current Space
#[derive(Resource, Default)]
pub struct SpaceFileRegistry {
    /// Map: file path → entity spawned from that file
    pub file_to_entity: HashMap<PathBuf, Entity>,
    
    /// Map: entity → file path it was loaded from
    pub entity_to_file: HashMap<Entity, PathBuf>,
    
    /// Map: file path → metadata
    pub file_metadata: HashMap<PathBuf, FileMetadata>,
    
    /// Files that failed to load (with error message)
    pub failed_files: HashMap<PathBuf, String>,
}

impl SpaceFileRegistry {
    /// Register a file and its spawned entity
    pub fn register(&mut self, path: PathBuf, entity: Entity, metadata: FileMetadata) {
        self.file_to_entity.insert(path.clone(), entity);
        self.entity_to_file.insert(entity, path.clone());
        self.file_metadata.insert(path, metadata);
    }
    
    /// Unregister a file (when deleted or entity despawned)
    pub fn unregister_file(&mut self, path: &Path) {
        if let Some(entity) = self.file_to_entity.remove(path) {
            self.entity_to_file.remove(&entity);
        }
        self.file_metadata.remove(path);
        self.failed_files.remove(path);
    }
    
    /// Unregister an entity
    pub fn unregister_entity(&mut self, entity: Entity) {
        if let Some(path) = self.entity_to_file.remove(&entity) {
            self.file_to_entity.remove(&path);
        }
    }
    
    /// Get entity for a file path
    pub fn get_entity(&self, path: &Path) -> Option<Entity> {
        self.file_to_entity.get(path).copied()
    }
    
    /// Get file path for an entity
    pub fn get_file(&self, entity: Entity) -> Option<&PathBuf> {
        self.entity_to_file.get(&entity)
    }
    
    /// Check if a file is loaded
    pub fn is_loaded(&self, path: &Path) -> bool {
        self.file_to_entity.contains_key(path)
    }
}

/// Component marking an entity as loaded from a file
#[derive(Component, Debug, Clone)]
pub struct LoadedFromFile {
    pub path: PathBuf,
    pub file_type: FileType,
    pub service: String,
}

/// Scan a single directory level, returning file entries and Directory entries
/// (each Directory entry carries its own children recursively).
fn scan_dir_entries(dir_path: &Path, service: &str) -> Vec<FileMetadata> {
    let mut entries: Vec<FileMetadata> = Vec::new();
    let Ok(read_dir) = std::fs::read_dir(dir_path) else { return entries };

    for entry in read_dir.flatten() {
        let path = entry.path();

        if path.is_dir() {
            // Recurse — build a Directory entry whose children are its contents
            let name = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string();
            let children = scan_dir_entries(&path, service);
            entries.push(FileMetadata {
                path: path.clone(),
                file_type: FileType::Directory,
                service: service.to_string(),
                name,
                size: 0,
                modified: std::time::SystemTime::UNIX_EPOCH,
                children,
            });
        } else {
            // Regular file
            let Some(file_type) = FileType::from_path(&path) else { continue };
            let Ok(meta) = std::fs::metadata(&path) else { continue };
            let name = path.file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string();
            entries.push(FileMetadata {
                path,
                file_type,
                service: service.to_string(),
                name,
                size: meta.len(),
                modified: meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                children: Vec::new(),
            });
        }
    }
    entries
}

/// Scan a Space directory and discover all loadable files and subdirectories.
/// Returns a flat list — Directory entries carry their children inline.
pub fn scan_space_directory(space_path: &Path) -> Vec<FileMetadata> {
    let mut files = Vec::new();
    
    // Services to scan
    let services = [
        "Workspace",
        "Lighting",
        "Players",
        "ServerStorage",
        "SoulService",
        "SoundService",
        "StarterCharacterScripts",
        "StarterGui",
        "StarterPack",
        "StarterPlayerScripts",
        "Teams",
    ];
    
    for service in &services {
        let service_path = space_path.join(service);
        if !service_path.exists() { continue; }
        files.extend(scan_dir_entries(&service_path, service));
    }
    
    files
}

/// Spawn a single file entry as an ECS entity, optionally parented to `parent_entity`.
/// Returns the spawned entity if one was created.
fn spawn_file_entry(
    commands: &mut Commands,
    asset_server: &Res<AssetServer>,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    registry: &mut ResMut<SpaceFileRegistry>,
    space_path: &Path,
    file_meta: &FileMetadata,
    parent_entity: Option<Entity>,
) -> Option<Entity> {
    // Skip if already loaded
    if registry.is_loaded(&file_meta.path) {
        return None;
    }

    // Check if this file type should spawn an entity in this service
    if !file_meta.file_type.spawns_entity_in_service(&file_meta.service) {
        debug!("Skipping {:?} in {} (doesn't spawn entity)", file_meta.path, file_meta.service);
        return None;
    }

    let entity = match file_meta.file_type {
        FileType::Toml => {
            match super::instance_loader::load_instance_definition(&file_meta.path) {
                Ok(instance) => {
                    let e = super::instance_loader::spawn_instance(
                        commands,
                        meshes,
                        materials,
                        space_path,
                        file_meta.path.clone(),
                        instance,
                    );
                    registry.register(file_meta.path.clone(), e, file_meta.clone());
                    e
                }
                Err(err) => {
                    error!("Failed to load instance file {:?}: {}", file_meta.path, err);
                    return None;
                }
            }
        }

        FileType::Gltf => {
            let scene_handle = asset_server.load(format!("{}#Scene0", file_meta.path.display()));
            let e = commands.spawn((
                SceneRoot(scene_handle),
                Transform::default(),
                eustress_common::classes::Instance {
                    name: file_meta.name.clone(),
                    class_name: eustress_common::classes::ClassName::Part,
                    archivable: true,
                    id: 0,
                    ai: false,
                },
                eustress_common::default_scene::PartEntityMarker {
                    part_id: file_meta.name.clone(),
                },
                LoadedFromFile {
                    path: file_meta.path.clone(),
                    file_type: file_meta.file_type,
                    service: file_meta.service.clone(),
                },
                Name::new(file_meta.name.clone()),
            )).id();
            registry.register(file_meta.path.clone(), e, file_meta.clone());
            info!("✅ Loaded {} from {:?}", file_meta.name, file_meta.path);
            e
        }

        FileType::Soul => {
            match std::fs::read_to_string(&file_meta.path) {
                Ok(markdown_source) => {
                    let e = commands.spawn((
                        eustress_common::classes::Instance {
                            name: file_meta.name.clone(),
                            class_name: eustress_common::classes::ClassName::SoulScript,
                            archivable: true,
                            id: 0,
                            ai: false,
                        },
                        crate::soul::SoulScriptData {
                            source: markdown_source,
                            dirty: false,
                            ast: None,
                            generated_code: None,
                            build_status: crate::soul::SoulBuildStatus::NotBuilt,
                            errors: Vec::new(),
                        },
                        LoadedFromFile {
                            path: file_meta.path.clone(),
                            file_type: file_meta.file_type,
                            service: file_meta.service.clone(),
                        },
                        Name::new(file_meta.name.clone()),
                    )).id();
                    registry.register(file_meta.path.clone(), e, file_meta.clone());
                    info!("📜 Loaded Soul script {} from {:?}", file_meta.name, file_meta.path);
                    e
                }
                Err(err) => {
                    error!("❌ Failed to read Soul script {:?}: {}", file_meta.path, err);
                    return None;
                }
            }
        }

        FileType::Rune => {
            debug!("Skipping .rune file (compiled bytecode): {:?}", file_meta.path);
            return None;
        }

        FileType::Ogg | FileType::Mp3 | FileType::Wav | FileType::Flac => {
            info!("🔊 Audio file discovered: {:?} (loader not yet implemented)", file_meta.path);
            return None;
        }

        _ => {
            debug!("File type {:?} loader not yet implemented", file_meta.file_type);
            return None;
        }
    };

    // Parent to Folder entity if provided
    if let Some(parent) = parent_entity {
        commands.entity(entity).insert(ChildOf(parent));
    }

    Some(entity)
}

/// Spawn a Directory entry as a Folder entity, then spawn all its children
/// parented to that Folder.  Recurses for nested subdirectories.
fn spawn_directory_entry(
    commands: &mut Commands,
    asset_server: &Res<AssetServer>,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    registry: &mut ResMut<SpaceFileRegistry>,
    space_path: &Path,
    dir_meta: &FileMetadata,
    parent_entity: Option<Entity>,
) {
    // Skip if this directory path already has an entity registered
    if registry.is_loaded(&dir_meta.path) {
        return;
    }

    // Spawn the Folder entity
    let folder_entity = commands.spawn((
        eustress_common::classes::Instance {
            name: dir_meta.name.clone(),
            class_name: eustress_common::classes::ClassName::Folder,
            archivable: true,
            id: 0,
            ai: false,
        },
        LoadedFromFile {
            path: dir_meta.path.clone(),
            file_type: FileType::Directory,
            service: dir_meta.service.clone(),
        },
        Name::new(dir_meta.name.clone()),
        Transform::default(),
        Visibility::default(),
    )).id();

    // Parent to containing Folder or service root if provided
    if let Some(parent) = parent_entity {
        commands.entity(folder_entity).insert(ChildOf(parent));
    }

    registry.register(dir_meta.path.clone(), folder_entity, dir_meta.clone());
    info!("📁 Spawned Folder '{}' ({} items)", dir_meta.name, dir_meta.children.len());

    // Spawn all children parented to this folder
    for child in &dir_meta.children {
        match child.file_type {
            FileType::Directory => {
                spawn_directory_entry(
                    commands, asset_server, meshes, materials, registry,
                    space_path, child, Some(folder_entity),
                );
            }
            _ => {
                spawn_file_entry(
                    commands, asset_server, meshes, materials, registry,
                    space_path, child, Some(folder_entity),
                );
            }
        }
    }
}

/// System to dynamically load all files in the Space
pub fn load_space_files_system(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut registry: ResMut<SpaceFileRegistry>,
) {
    // Get Space path (TODO: make this configurable)
    let space_path = PathBuf::from("C:/Users/miksu/Documents/Eustress/Universe1/spaces/Space1");
    
    if !space_path.exists() {
        warn!("Space path does not exist: {:?}", space_path);
        return;
    }
    
    // Scan for files and directories
    let entries = scan_space_directory(&space_path);
    info!("🔍 Discovered {} top-level entries in Space", entries.len());
    
    for entry in &entries {
        match entry.file_type {
            // Subdirectory → Folder entity + children parented to it
            FileType::Directory => {
                spawn_directory_entry(
                    &mut commands, &asset_server, &mut meshes, &mut materials,
                    &mut registry, &space_path, entry, None,
                );
            }
            // Regular file → entity at service root level (no parent)
            _ => {
                spawn_file_entry(
                    &mut commands, &asset_server, &mut meshes, &mut materials,
                    &mut registry, &space_path, entry, None,
                );
            }
        }
    }
}

/// Plugin for dynamic file loading
pub struct SpaceFileLoaderPlugin;

impl Plugin for SpaceFileLoaderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpaceFileRegistry>()
            .add_systems(Startup, (
                load_space_files_system.after(crate::default_scene::setup_default_scene),
                super::file_watcher::setup_file_watcher,
            ))
            .add_systems(Update, (
                super::file_watcher::process_file_changes,
                super::instance_loader::write_instance_changes_system,
            ));
    }
}
