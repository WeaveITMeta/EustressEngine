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
    
    // GUI Elements (StarterGui)
    GuiElement,     // .textlabel, .textbutton, .frame, .imagelabel, .imagebutton, .scrollingframe
    
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
            // GUI elements
            "textlabel" | "textbutton" | "frame" | "imagelabel" | "imagebutton" | 
            "scrollingframe" | "textbox" | "viewportframe" => Some(Self::GuiElement),
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "ron" => Some(Self::Ron),
            _ => None,
        }
    }
    
    /// Get file type from full path (handles compound extensions like .glb.toml, .part.toml)
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        let path_str = path.to_string_lossy();
        
        // Check for compound extensions first (order matters - check specific before generic)
        
        // EEP marker files (folder containers per EEP_SPECIFICATION.md)
        // _service.toml - marks a folder as a Service (Workspace, Lighting, etc.)
        // _instance.toml - marks a folder as a container (Model, Folder, ScreenGui, etc.)
        if path_str.ends_with("_service.toml") || path_str.ends_with("_instance.toml") {
            return Some(Self::Toml); // Container marker file
        }
        
        // Instance files (spawn as entities)
        if path_str.ends_with(".glb.toml") 
            || path_str.ends_with(".part.toml") 
            || path_str.ends_with(".model.toml")
            || path_str.ends_with(".instance.toml") 
        {
            return Some(Self::Toml); // Instance file
        }
        // Scene files
        if path_str.ends_with(".scene.toml") {
            return Some(Self::Scene);
        }
        // Material files
        if path_str.ends_with(".mat.toml") {
            return Some(Self::Material);
        }
        // GUI element compound extensions (.textlabel.toml, .textbutton.toml, .frame.toml, etc.)
        // Must be checked BEFORE the plain .toml catch-all below
        if path_str.ends_with(".textlabel.toml") || path_str.ends_with(".textbutton.toml")
            || path_str.ends_with(".frame.toml") || path_str.ends_with(".imagelabel.toml")
            || path_str.ends_with(".imagebutton.toml") || path_str.ends_with(".scrollingframe.toml")
            || path_str.ends_with(".textbox.toml") || path_str.ends_with(".viewportframe.toml")
            || path_str.ends_with(".screengui.toml")
        {
            return Some(Self::GuiElement);
        }
        
        // Plain .toml files (config, settings, etc.) - don't spawn entities
        if path_str.ends_with(".toml") {
            return None; // Ignore plain .toml files - they're config, not instances
        }
        
        // Fall back to simple extension check for non-TOML files
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(Self::from_extension)
    }
    
    /// Check if this file type should spawn an entity in the given service folder
    pub fn spawns_entity_in_service(&self, service: &str) -> bool {
        match (self, service) {
            // Workspace: Instance files (.glb.toml, .part.toml, .model.toml, .instance.toml, _instance.toml, _service.toml) and 3D models spawn as Parts
            (Self::Toml | Self::Gltf | Self::Obj | Self::Fbx, "Workspace") => true,
            
            // Lighting: Models can be light sources
            (Self::Gltf | Self::Obj | Self::Fbx, "Lighting") => true,
            
            // SoulService: Scripts don't spawn visible entities, but need to be loaded
            (Self::Soul | Self::Rune | Self::Wasm | Self::Lua, "SoulService") => true,
            
            // SoundService: Audio files spawn as Sound entities
            (Self::Ogg | Self::Mp3 | Self::Wav | Self::Flac, "SoundService") => true,
            
            // StarterGui: GUI elements spawn as UI entities
            (Self::GuiElement | Self::Toml, "StarterGui") => true,
            
            // Scripts in any service folder
            (Self::Soul | Self::Rune, _) => true,
            
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
/// 
/// Services are discovered from the filesystem by looking for directories
/// containing `_service.toml` marker files (EEP-compliant, no hardcoding).
pub fn scan_space_directory(space_path: &Path) -> Vec<FileMetadata> {
    let mut files = Vec::new();
    
    // Discover services from filesystem - look for directories with _service.toml
    // This replaces the hardcoded service list with EEP-compliant discovery
    let services = discover_services(space_path);
    
    for service_name in &services {
        let service_path = space_path.join(service_name);
        if !service_path.exists() { continue; }
        files.extend(scan_dir_entries(&service_path, service_name));
    }
    
    files
}

/// Discover services by scanning for directories containing `_service.toml` marker files.
/// This is EEP-compliant: services are defined by filesystem structure, not hardcoded.
fn discover_services(space_path: &Path) -> Vec<String> {
    let mut services = Vec::new();
    
    let entries = match std::fs::read_dir(space_path) {
        Ok(entries) => entries,
        Err(e) => {
            warn!("Failed to read Space directory {:?}: {}", space_path, e);
            return services;
        }
    };
    
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() { continue; }
        
        // Check if this directory contains a _service.toml marker file
        let service_marker = path.join("_service.toml");
        if service_marker.exists() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                services.push(name.to_string());
                debug!("Discovered service: {} (has _service.toml)", name);
            }
        }
    }
    
    // Sort for deterministic order (Workspace first for consistency)
    services.sort_by(|a, b| {
        if a == "Workspace" { std::cmp::Ordering::Less }
        else if b == "Workspace" { std::cmp::Ordering::Greater }
        else { a.cmp(b) }
    });
    
    info!("📁 Discovered {} services from filesystem: {:?}", services.len(), services);
    services
}

/// Spawn a single file entry as an ECS entity, optionally parented to `parent_entity`.
/// Returns the spawned entity if one was created.
pub fn spawn_file_entry(
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

    // Skip files inside 'meshes' folders - these are raw assets, not parts to spawn
    // They are referenced by .glb.toml or .part.toml instance files instead
    if file_meta.path.components().any(|c| c.as_os_str() == "meshes") {
        debug!("Skipping {:?} (inside meshes folder - raw asset)", file_meta.path);
        return None;
    }

    // Check if this file type should spawn an entity in this service
    if !file_meta.file_type.spawns_entity_in_service(&file_meta.service) {
        debug!("Skipping {:?} in {} (doesn't spawn entity)", file_meta.path, file_meta.service);
        return None;
    }

    let entity = match file_meta.file_type {
        FileType::Toml => {
            // Check if this is a _service.toml file (service marker)
            let is_service = file_meta.path.file_name()
                .map(|n| n.to_string_lossy().ends_with("_service.toml"))
                .unwrap_or(false);
            
            if is_service {
                // Load as service entity
                match super::service_loader::load_service_definition(&file_meta.path) {
                    Ok(service_def) => {
                        let e = super::service_loader::spawn_service(
                            commands,
                            file_meta.path.clone(),
                            service_def,
                        );
                        registry.register(file_meta.path.clone(), e, file_meta.clone());
                        e
                    }
                    Err(err) => {
                        error!("Failed to load service file {:?}: {}", file_meta.path, err);
                        return None;
                    }
                }
            } else {
                // Load as instance entity
                match super::instance_loader::load_instance_definition(&file_meta.path) {
                    Ok(instance) => {
                        let e = super::instance_loader::spawn_instance(
                            commands,
                            asset_server,
                            materials,
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
        }

        FileType::Gltf => {
            // Check for Draco compression before loading
            if super::draco_decoder::is_draco_compressed(&file_meta.path) {
                super::draco_decoder::warn_draco_file(&file_meta.path);
                return None; // Skip loading Draco-compressed files
            }
            
            // Use space:// asset source for GLB files in the Space directory
            let space_root = super::default_space_root();
            let relative_path = file_meta.path
                .strip_prefix(&space_root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| file_meta.path.to_string_lossy().replace('\\', "/"));
            let asset_path = format!("space://{}#Scene0", relative_path);
            info!("🔧 Loading GLTF: {} (from {:?})", asset_path, file_meta.path);
            let scene_handle = asset_server.load(asset_path);
            let e = commands.spawn((
                SceneRoot(scene_handle),
                Transform::default(),
                Visibility::default(),
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
            // Load .rune files as SoulScript entities (Rune is the scripting language)
            match std::fs::read_to_string(&file_meta.path) {
                Ok(rune_source) => {
                    let e = commands.spawn((
                        eustress_common::classes::Instance {
                            name: file_meta.name.clone(),
                            class_name: eustress_common::classes::ClassName::SoulScript,
                            archivable: true,
                            id: 0,
                            ai: false,
                        },
                        crate::soul::SoulScriptData {
                            source: rune_source,
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
                    info!("📜 Loaded Rune script {} from {:?}", file_meta.name, file_meta.path);
                    e
                }
                Err(err) => {
                    error!("❌ Failed to read Rune script {:?}: {}", file_meta.path, err);
                    return None;
                }
            }
        }

        FileType::GuiElement => {
            // Load GUI element files (.textlabel.toml, .frame.toml, etc.) as UI entities.
            // path.extension() returns "toml" for compound paths, so extract the
            // second-to-last stem segment (e.g. "Panel.frame.toml" → stem "Panel.frame" → ext "frame").
            let gui_ext = file_meta.path
                .file_stem()  // "Panel.frame"
                .and_then(|s| std::path::Path::new(s).extension())  // "frame"
                .and_then(|e| e.to_str())
                .unwrap_or("");
            let class_name = match gui_ext {
                "textlabel" => eustress_common::classes::ClassName::TextLabel,
                "textbutton" => eustress_common::classes::ClassName::TextButton,
                "frame" | "screengui" => eustress_common::classes::ClassName::Frame,
                "imagelabel" => eustress_common::classes::ClassName::ImageLabel,
                "imagebutton" => eustress_common::classes::ClassName::ImageButton,
                "scrollingframe" => eustress_common::classes::ClassName::ScrollingFrame,
                "textbox" => eustress_common::classes::ClassName::TextBox,
                "viewportframe" => eustress_common::classes::ClassName::ViewportFrame,
                _ => eustress_common::classes::ClassName::Frame, // Default to Frame
            };
            // Name is everything before the first dot (e.g. "Panel" from "Panel.frame.toml")
            let display_name = file_meta.path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.splitn(2, '.').next())
                .unwrap_or(&file_meta.name)
                .to_string();
            
            let e = commands.spawn((
                eustress_common::classes::Instance {
                    name: display_name.clone(),
                    class_name,
                    archivable: true,
                    id: 0,
                    ai: false,
                },
                LoadedFromFile {
                    path: file_meta.path.clone(),
                    file_type: file_meta.file_type,
                    service: file_meta.service.clone(),
                },
                Name::new(display_name.clone()),
            )).id();
            registry.register(file_meta.path.clone(), e, file_meta.clone());
            info!("🖼️ Loaded GUI element {} ({:?}) from {:?}", display_name, class_name, file_meta.path);
            e
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
pub fn spawn_directory_entry(
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

    // Skip 'meshes' directories - these are asset storage, not part of the scene hierarchy
    if dir_meta.name == "meshes" || dir_meta.path.components().any(|c| c.as_os_str() == "meshes") {
        debug!("Skipping meshes directory {:?} (asset storage)", dir_meta.path);
        return;
    }

    // Check for _instance.toml — it may declare a richer class (e.g. ScreenGui)
    let instance_toml_path = dir_meta.path.join("_instance.toml");
    let class_name = if instance_toml_path.exists() {
        std::fs::read_to_string(&instance_toml_path)
            .ok()
            .and_then(|s| toml::from_str::<toml::Value>(&s).ok())
            .and_then(|v| v.get("metadata").and_then(|m| m.get("class_name")).and_then(|c| c.as_str()).map(|s| s.to_string()))
            .map(|cn| match cn.as_str() {
                "ScreenGui"      => eustress_common::classes::ClassName::ScreenGui,
                "Frame"          => eustress_common::classes::ClassName::Frame,
                "ScrollingFrame" => eustress_common::classes::ClassName::ScrollingFrame,
                "BillboardGui"   => eustress_common::classes::ClassName::BillboardGui,
                "SurfaceGui"     => eustress_common::classes::ClassName::SurfaceGui,
                "Model"          => eustress_common::classes::ClassName::Model,
                _                => eustress_common::classes::ClassName::Folder,
            })
            .unwrap_or(eustress_common::classes::ClassName::Folder)
    } else {
        eustress_common::classes::ClassName::Folder
    };

    // Spawn the Folder / ScreenGui / Model entity
    let folder_entity = commands.spawn((
        eustress_common::classes::Instance {
            name: dir_meta.name.clone(),
            class_name,
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
    space_root: Res<super::SpaceRoot>,
) {
    let space_path = &space_root.0;
    
    if !space_path.exists() {
        warn!("Space path does not exist: {:?}", space_path);
        return;
    }
    
    // Scan for files and directories
    let entries = scan_space_directory(space_path);
    info!("🔍 Discovered {} top-level entries in Space", entries.len());
    
    for entry in &entries {
        match entry.file_type {
            // Subdirectory → Folder entity + children parented to it
            FileType::Directory => {
                spawn_directory_entry(
                    &mut commands, &asset_server, &mut meshes, &mut materials,
                    &mut registry, space_path, entry, None,
                );
            }
            // Regular file → entity at service root level (no parent)
            _ => {
                spawn_file_entry(
                    &mut commands, &asset_server, &mut meshes, &mut materials,
                    &mut registry, space_path, entry, None,
                );
            }
        }
    }
}

/// Plugin for dynamic file loading
pub struct SpaceFileLoaderPlugin;

impl Plugin for SpaceFileLoaderPlugin {
    fn build(&self, app: &mut App) {
        // Note: The "space://" asset source is registered in main.rs BEFORE DefaultPlugins
        // This must happen before AssetPlugin is initialized, so we can't do it here.
        
        app.init_resource::<super::SpaceRoot>()
            .init_resource::<SpaceFileRegistry>()
            .init_resource::<super::file_watcher::RecentlyWrittenFiles>()
            .init_resource::<super::space_ops::SpaceRescanNeeded>()
            .add_systems(Startup, (
                load_space_files_system.after(crate::default_scene::setup_default_scene),
                super::file_watcher::setup_file_watcher,
            ))
            .add_systems(Update, (
                super::file_watcher::process_file_changes,
                super::instance_loader::write_instance_changes_system,
                super::space_ops::apply_space_rescan,
            ));
    }
}
