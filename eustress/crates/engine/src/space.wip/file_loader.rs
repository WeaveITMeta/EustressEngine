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

/// Metadata extracted from a file
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub path: PathBuf,
    pub file_type: FileType,
    pub service: String,
    pub name: String,
    pub size: u64,
    pub modified: std::time::SystemTime,
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

/// Scan a Space directory and discover all loadable files
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
        if !service_path.exists() {
            continue;
        }
        
        // Recursively scan service folder
        if let Ok(entries) = std::fs::read_dir(&service_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                
                // Skip directories for now (TODO: recursive scan)
                if path.is_dir() {
                    continue;
                }
                
                // Determine file type (use from_path to handle compound extensions)
                let Some(file_type) = FileType::from_path(&path) else {
                    continue;
                };
                
                // Get file metadata
                let Ok(metadata) = std::fs::metadata(&path) else {
                    continue;
                };
                
                let name = path.file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Unknown")
                    .to_string();
                
                files.push(FileMetadata {
                    path,
                    file_type,
                    service: service.to_string(),
                    name,
                    size: metadata.len(),
                    modified: metadata.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                });
            }
        }
    }
    
    files
}

/// System to dynamically load all files in the Space
pub fn load_space_files_system(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut registry: ResMut<SpaceFileRegistry>,
) {
    // Get Space path (TODO: make this configurable)
    let space_path = PathBuf::from("C:/Users/miksu/Documents/Eustress/Universe1/spaces/Space1");
    
    if !space_path.exists() {
        warn!("Space path does not exist: {:?}", space_path);
        return;
    }
    
    // Scan for files
    let files = scan_space_directory(&space_path);
    
    info!("🔍 Discovered {} loadable files in Space", files.len());
    
    // Load each file
    for file_meta in files {
        // Skip if already loaded
        if registry.is_loaded(&file_meta.path) {
            continue;
        }
        
        // Check if this file type should spawn an entity in this service
        if !file_meta.file_type.spawns_entity_in_service(&file_meta.service) {
            debug!("Skipping {:?} in {} (doesn't spawn entity)", file_meta.path, file_meta.service);
            continue;
        }
        
        // Load based on file type
        match file_meta.file_type {
            FileType::Toml => {
                // Load .glb.toml instance file
                match super::instance_loader::load_instance_definition(&file_meta.path) {
                    Ok(instance) => {
                        let entity = super::instance_loader::spawn_instance(
                            &mut commands,
                            &asset_server,
                            &space_path,
                            file_meta.path.clone(),
                            instance,
                        );
                        
                        registry.register(file_meta.path.clone(), entity, file_meta.clone());
                    }
                    Err(e) => {
                        error!("Failed to load instance file {:?}: {}", file_meta.path, e);
                    }
                }
            }
            
            FileType::Gltf => {
                // Load glTF/GLB file directly (legacy, prefer .glb.toml instances)
                let scene_handle = asset_server.load(format!("{}#Scene0", file_meta.path.display()));
                
                let entity = commands.spawn((
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
                
                registry.register(file_meta.path.clone(), entity, file_meta.clone());
                info!("✅ Loaded {} from {:?}", file_meta.name, file_meta.path);
            }
            
            FileType::Soul => {
                // Load Soul script (.md file that compiles to .rune in cache)
                // Read the markdown source
                match std::fs::read_to_string(&file_meta.path) {
                    Ok(markdown_source) => {
                        // Create SoulScript entity with the markdown source
                        let entity = commands.spawn((
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
                        
                        registry.register(file_meta.path.clone(), entity, file_meta.clone());
                        info!("📜 Loaded Soul script {} from {:?}", file_meta.name, file_meta.path);
                        
                        // Note: The Soul build pipeline will automatically compile this to .rune
                        // when the user triggers a build or when auto-build is enabled
                    }
                    Err(e) => {
                        error!("❌ Failed to read Soul script {:?}: {}", file_meta.path, e);
                    }
                }
            }
            
            FileType::Rune => {
                // .rune files are compiled bytecode in cache - skip for now
                // These are generated by the Soul build pipeline
                debug!("Skipping .rune file (compiled bytecode): {:?}", file_meta.path);
            }
            
            FileType::Ogg | FileType::Mp3 | FileType::Wav | FileType::Flac => {
                // Load audio file
                // TODO: implement audio entity spawning
                info!("🔊 Audio file discovered: {:?} (loader not yet implemented)", file_meta.path);
            }
            
            _ => {
                debug!("File type {:?} loader not yet implemented", file_meta.file_type);
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
