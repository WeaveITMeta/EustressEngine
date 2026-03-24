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
        // These are metadata files, NOT entities — return None so they are never spawned.
        if path_str.ends_with("_service.toml") || path_str.ends_with("_instance.toml") {
            return None;
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
            
            // MaterialService: Material definitions spawn as material entities
            (Self::Material, "MaterialService") => true,
            
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

            // Skip EEP marker files — these define folder types, not entities
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if fname == "_instance.toml" || fname == "_service.toml" {
                continue;
            }

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
/// Returns service directories as Directory entries with their children inline.
/// 
/// Services are discovered from the filesystem by looking for directories
/// containing `_service.toml` marker files (EEP-compliant, no hardcoding).
pub fn scan_space_directory(space_path: &Path) -> Vec<FileMetadata> {
    let mut entries = Vec::new();
    
    // Discover services from filesystem - look for directories with _service.toml
    // This replaces the hardcoded service list with EEP-compliant discovery
    let services = discover_services(space_path);
    
    for service_name in &services {
        let service_path = space_path.join(service_name);
        if !service_path.exists() { continue; }
        
        // Return the service directory as a Directory entry with its contents as children
        // This ensures files inside services (like materials) get parented to the service entity
        let children = scan_dir_entries(&service_path, service_name);
        entries.push(FileMetadata {
            path: service_path,
            file_type: FileType::Directory,
            service: service_name.clone(),
            name: service_name.clone(),
            size: 0,
            modified: std::time::SystemTime::UNIX_EPOCH,
            children,
        });
    }
    
    entries
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
    material_registry: &mut ResMut<super::material_loader::MaterialRegistry>,
    mesh_cache: &mut ResMut<super::instance_loader::PrimitiveMeshCache>,
    space_path: &Path,
    file_meta: &FileMetadata,
    parent_entity: Option<Entity>,
    class_defaults: Option<&super::class_defaults::ClassDefaultsRegistry>,
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

    // Skip _instance.toml marker files — they define the parent folder type,
    // not entities to render in the Explorer tree
    let is_instance_marker = file_meta.path.file_name()
        .map(|n| n.to_string_lossy() == "_instance.toml")
        .unwrap_or(false);
    if is_instance_marker {
        debug!("Skipping {:?} (folder container marker, not an entity)", file_meta.path);
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
                match super::instance_loader::load_instance_definition_with_defaults(&file_meta.path, class_defaults) {
                    Ok(instance) => {
                        let e = super::instance_loader::spawn_instance(
                            commands,
                            asset_server,
                            materials,
                            material_registry,
                            mesh_cache,
                            file_meta.path.clone(),
                            instance,
                        );
                        // Attach LoadedFromFile so the Explorer can classify this
                        // entity by service (Workspace, Lighting, etc.)
                        commands.entity(e).insert(LoadedFromFile {
                            path: file_meta.path.clone(),
                            file_type: file_meta.file_type,
                            service: file_meta.service.clone(),
                        });
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
                            run_context: Default::default(),
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
                            run_context: Default::default(),
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
            // Load GUI element files (.textlabel.toml, .frame.toml, etc.) as Bevy UI entities.
            // Parses the TOML file for visual properties (position, size, colors, text)
            // and spawns with proper Bevy UI components (Node, BackgroundColor, Text, etc.)
            // so they render visually in the viewport.
            match super::gui_loader::load_gui_definition(&file_meta.path) {
                Ok(gui_def) => {
                    let display_name = super::gui_loader::gui_display_name(&file_meta.path);
                    let gui_type = super::gui_loader::gui_class_from_extension(&file_meta.path);
                    let e = super::gui_loader::spawn_gui_element(
                        commands,
                        &file_meta.path,
                        &gui_def,
                    );
                    registry.register(file_meta.path.clone(), e, file_meta.clone());
                    info!("🖼️ Loaded GUI element {} ({}) from {:?}", display_name, gui_type, file_meta.path);
                    e
                }
                Err(err) => {
                    error!("Failed to load GUI element {:?}: {}", file_meta.path, err);
                    return None;
                }
            }
        }

        FileType::Material => {
            // Load .mat.toml files into MaterialRegistry and spawn Explorer entity
            match super::material_loader::load_material_definition(&file_meta.path) {
                Ok(definition) => {
                    let mat_name = if definition.material.name.is_empty() {
                        super::material_loader::material_name_from_path(&file_meta.path)
                    } else {
                        definition.material.name.clone()
                    };
                    let mat_toml_dir = file_meta.path.parent().unwrap_or(std::path::Path::new("."));
                    let standard_mat = super::material_loader::build_standard_material(
                        &definition,
                        asset_server,
                        mat_toml_dir,
                        space_path,
                    );
                    let handle = materials.add(standard_mat);
                    // Register material in the MaterialRegistry for Part resolution
                    material_registry.insert(
                        mat_name.clone(),
                        handle,
                        definition.clone(),
                        file_meta.path.clone(),
                    );
                    let e = super::material_loader::spawn_material_entity(
                        commands,
                        file_meta.path.clone(),
                        &definition,
                    );
                    registry.register(file_meta.path.clone(), e, file_meta.clone());
                    info!("🎨 Loaded material '{}' from {:?}", mat_name, file_meta.path);
                    e
                }
                Err(err) => {
                    error!("Failed to load material {:?}: {}", file_meta.path, err);
                    return None;
                }
            }
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

/// Spawn a Directory entry as a Folder entity (or Service entity if it contains _service.toml),
/// then spawn all its children parented to that entity. Recurses for nested subdirectories.
pub fn spawn_directory_entry(
    commands: &mut Commands,
    asset_server: &Res<AssetServer>,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    registry: &mut ResMut<SpaceFileRegistry>,
    material_registry: &mut ResMut<super::material_loader::MaterialRegistry>,
    mesh_cache: &mut ResMut<super::instance_loader::PrimitiveMeshCache>,
    space_path: &Path,
    dir_meta: &FileMetadata,
    parent_entity: Option<Entity>,
    class_defaults: Option<&super::class_defaults::ClassDefaultsRegistry>,
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

    // Check for _service.toml — this is a service root directory
    let service_toml_path = dir_meta.path.join("_service.toml");
    if service_toml_path.exists() {
        // Spawn as a Service entity, then parent children to it
        match super::service_loader::load_service_definition(&service_toml_path) {
            Ok(service_def) => {
                let service_entity = super::service_loader::spawn_service(
                    commands,
                    service_toml_path.clone(),
                    service_def,
                );
                registry.register(service_toml_path, service_entity, dir_meta.clone());
                info!("🏢 Spawned Service '{}' with {} children", dir_meta.name, dir_meta.children.len());
                
                // Spawn all children parented to this service
                for child in &dir_meta.children {
                    match child.file_type {
                        FileType::Directory => {
                            spawn_directory_entry(
                                commands, asset_server, meshes, materials, registry,
                                material_registry, mesh_cache, space_path, child, Some(service_entity),
                                class_defaults,
                            );
                        }
                        _ => {
                            spawn_file_entry(
                                commands, asset_server, meshes, materials, registry,
                                material_registry, mesh_cache, space_path, child, Some(service_entity),
                                class_defaults,
                            );
                        }
                    }
                }
            }
            Err(err) => {
                error!("Failed to load service {:?}: {}", service_toml_path, err);
            }
        }
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
    // ScreenGui directories need Bevy UI components (Node, GlobalZIndex) instead
    // of 3D components (Transform, Visibility) so child UI elements render correctly.
    let is_screen_gui = matches!(class_name, eustress_common::classes::ClassName::ScreenGui);

    let folder_entity = if is_screen_gui {
        // ScreenGui: fullscreen UI root — uses Node layout so children render as Bevy UI
        commands.spawn((
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
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                ..default()
            },
            GlobalZIndex(100), // Above 3D scene, below Slint overlay
            BackgroundColor(Color::NONE),
        )).id()
    } else {
        // Regular Folder / Model — 3D entity
        commands.spawn((
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
        )).id()
    };

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
                    material_registry, mesh_cache, space_path, child, Some(folder_entity),
                    class_defaults,
                );
            }
            _ => {
                spawn_file_entry(
                    commands, asset_server, meshes, materials, registry,
                    material_registry, mesh_cache, space_path, child, Some(folder_entity),
                    class_defaults,
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
    mut material_registry: ResMut<super::material_loader::MaterialRegistry>,
    mut mesh_cache: ResMut<super::instance_loader::PrimitiveMeshCache>,
    space_root: Res<super::SpaceRoot>,
    class_defaults: Option<Res<super::class_defaults::ClassDefaultsRegistry>>,
) {
    let space_path = &space_root.0;
    
    if !space_path.exists() {
        warn!("Space path does not exist: {:?}", space_path);
        return;
    }
    
    // Scan for files and directories
    let entries = scan_space_directory(space_path);
    info!("🔍 Discovered {} top-level entries in Space", entries.len());
    
    let cd_ref = class_defaults.as_deref();
    for entry in &entries {
        match entry.file_type {
            // Subdirectory → Folder entity + children parented to it
            FileType::Directory => {
                spawn_directory_entry(
                    &mut commands, &asset_server, &mut meshes, &mut materials,
                    &mut registry, &mut material_registry, &mut mesh_cache, space_path, entry, None,
                    cd_ref,
                );
            }
            // Regular file → entity at service root level (no parent)
            _ => {
                spawn_file_entry(
                    &mut commands, &asset_server, &mut meshes, &mut materials,
                    &mut registry, &mut material_registry, &mut mesh_cache, space_path, entry, None,
                    cd_ref,
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
            .init_resource::<super::material_loader::MaterialRegistry>()
            .init_resource::<super::instance_loader::PrimitiveMeshCache>()
            .init_resource::<super::file_watcher::RecentlyWrittenFiles>()
            .init_resource::<super::space_ops::SpaceRescanNeeded>()
            .add_systems(Startup, (
                super::class_defaults::startup_load_class_defaults,
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
