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
            "lua" | "luau" => Some(Self::Lua),
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

        // `_service.toml` — service marker, never an entity.
        if path_str.ends_with("_service.toml") {
            return None;
        }

        // `_instance.toml` — folder-based entity marker. On INITIAL SCAN
        // the folder walker classifies the parent directory via
        // `scan_dir_entries` + `spawn_directory_entry`, so the marker
        // itself is skipped by filename inside that path
        // (scan_dir_entries line ~329). But on HOT-CREATE we need this
        // file to route through the file-watcher's FileType::Toml
        // branch — otherwise a brand-new `Workspace/Foo/_instance.toml`
        // written by a Workshop tool (create_entity, etc.) is dropped
        // by `process_event`'s `FileType::from_path(...)?` and the
        // entity never spawns until the next full rescan. Returning
        // `Some(Self::Toml)` makes `load_instance_definition_with_defaults`
        // read this file directly; its InstanceDefinition captures the
        // same data the folder-walker would have derived.
        if path_str.ends_with("_instance.toml") {
            return Some(Self::Toml);
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
            
            // Lighting: TOML instance files (Sun, Moon, Sky, Atmosphere, Skybox)
            // spawn as non-visual Instance entities; hydrate_lighting_entities
            // attaches the real ECS components (DirectionalLight, markers, etc.)
            (Self::Toml | Self::Gltf | Self::Obj | Self::Fbx, "Lighting") => true,
            
            // SoulService: Scripts don't spawn visible entities, but need to be loaded
            (Self::Soul | Self::Rune | Self::Wasm | Self::Lua, "SoulService") => true,
            
            // SoundService: Audio files spawn as Sound entities
            (Self::Ogg | Self::Mp3 | Self::Wav | Self::Flac, "SoundService") => true,
            
            // StarterGui: GUI elements spawn as UI entities
            (Self::GuiElement | Self::Toml, "StarterGui") => true,

            // Workspace: GUI elements spawn here too — a BillboardGui /
            // SurfaceGui parented to a Part (e.g. SimpleBlock/Label/) can
            // have child UI elements (TextLabel, Frame, ImageLabel, …)
            // serialised as `.textlabel.toml` / etc. inside the same
            // folder. Without this arm, the file walker recurses into the
            // BillboardGui's directory, finds the child file, but
            // `spawns_entity_in_service` returns false → the child is
            // silently skipped → the Explorer reloads the BillboardGui
            // empty and the rendered billboard has no content. Mirror the
            // StarterGui rule so file-system-first parity holds wherever
            // the user attaches a GUI tree.
            (Self::GuiElement, "Workspace") => true,
            
            // MaterialService: Material definitions + texture images
            (Self::Material | Self::Png | Self::Jpg, "MaterialService") => true,

            // AdornmentService: Adornment definition TOMLs spawn as adornment entities
            (Self::Toml, "AdornmentService") => true,

            // Scripts in any service folder — Rune, Soul (markdown), and Luau
            // all spawn as SoulScript entities wherever the user drops them.
            (Self::Soul | Self::Rune | Self::Lua, _) => true,
            
            // Default: don't spawn
            _ => false,
        }
    }
}

/// Read a file's text through the active [`SpaceSource`], deriving the
/// Space-relative key from the absolute `abs_path` the loader still
/// carries for identity. Falls back to a direct `std::fs` read if the
/// path can't be made relative (defensive — should not happen for
/// in-Space content). This is the single seam every loader read site
/// goes through so Disk vs Fjall is one decision, not 18.
fn src_read_string(
    source: &dyn super::space_source::SpaceSource,
    space_root: &Path,
    abs_path: &Path,
) -> std::io::Result<String> {
    match super::space_source::rel_from_root(space_root, abs_path) {
        Some(rel) => source.read_to_string(&rel),
        None => std::fs::read_to_string(abs_path),
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

    /// Paths currently being renamed — file watcher ignores delete events for these.
    /// Cleared after the rename completes.
    pub rename_in_progress: std::collections::HashSet<PathBuf>,
}

impl SpaceFileRegistry {
    /// Register a file and its spawned entity
    pub fn register(&mut self, path: PathBuf, entity: Entity, metadata: FileMetadata) {
        self.file_to_entity.insert(path.clone(), entity);
        self.entity_to_file.insert(entity, path.clone());
        self.file_metadata.insert(path, metadata);
    }
    
    /// Clear all registry entries (used when switching spaces).
    pub fn clear(&mut self) {
        self.file_to_entity.clear();
        self.entity_to_file.clear();
        self.file_metadata.clear();
        self.failed_files.clear();
        self.rename_in_progress.clear();
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
    
    /// Rename a file in the registry — updates all maps to point to the new path.
    /// Returns Ok(()) on success, Err with message on failure.
    pub fn rename_file(&mut self, old_path: &Path, new_path: PathBuf) -> Result<(), String> {
        let entity = self.file_to_entity.remove(old_path)
            .ok_or_else(|| format!("No entity registered for {:?}", old_path))?;

        // Update all maps
        self.file_to_entity.insert(new_path.clone(), entity);
        self.entity_to_file.insert(entity, new_path.clone());

        if let Some(mut meta) = self.file_metadata.remove(old_path) {
            meta.path = new_path.clone();
            self.file_metadata.insert(new_path, meta);
        }

        Ok(())
    }

    /// Check if a file is loaded
    pub fn is_loaded(&self, path: &Path) -> bool {
        self.file_to_entity.contains_key(path)
    }

    /// Collect every (file_path, entity) entry whose path is a
    /// descendant of `ancestor`. Used by the delete handler when a
    /// parent folder gets renamed to trash so all descendant entities
    /// can be despawned together AND added to `rename_in_progress` —
    /// otherwise a child's still-pending save-on-Changed flushes after
    /// the parent rename, fails (parent dir gone), and can leave the
    /// engine and disk state subtly inconsistent. Match is strict
    /// `starts_with` on path components; the ancestor itself is
    /// included.
    pub fn descendants_of(&self, ancestor: &Path) -> Vec<(PathBuf, Entity)> {
        self.file_to_entity.iter()
            .filter(|(p, _)| p.starts_with(ancestor))
            .map(|(p, e)| (p.clone(), *e))
            .collect()
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
/// Recursively scan a directory's entries via [`SpaceSource`] rather
/// than `std::fs`. `rel_dir` is the Space-relative forward-slash path
/// of the directory being scanned; `space_root` reconstitutes the
/// absolute `FileMetadata.path` so the registry / file-watcher / undo
/// (all keyed on absolute paths) keep working unchanged — only the
/// *content + listing* moves to the source (Disk or Fjall).
fn scan_dir_entries(
    source: &dyn super::space_source::SpaceSource,
    space_root: &Path,
    rel_dir: &str,
    service: &str,
) -> Vec<FileMetadata> {
    let mut entries: Vec<FileMetadata> = Vec::new();
    let Ok(listing) = source.list(rel_dir) else { return entries };

    for ent in listing {
        let name = ent.name.clone();
        let rel = ent.rel_path.clone();
        // Absolute path kept for registry/watcher/undo identity.
        let path = {
            let mut p = space_root.to_path_buf();
            for seg in rel.split('/') {
                if !seg.is_empty() {
                    p.push(seg);
                }
            }
            p
        };

        if ent.is_dir {
            // Skip hidden/system directories (.eustress, .git, node_modules, target, trash)
            if name.starts_with('.') || name == "node_modules" || name == "target" || name == "trash" {
                continue;
            }
            // EEP reserved names — `_instance.toml` and `_service.toml`
            // are MARKER FILE names, never valid as directory names.
            if name == "_instance.toml" || name == "_service.toml" {
                continue;
            }
            let children = scan_dir_entries(source, space_root, &rel, service);
            entries.push(FileMetadata {
                path,
                file_type: FileType::Directory,
                service: service.to_string(),
                name,
                size: 0,
                modified: std::time::SystemTime::UNIX_EPOCH,
                children,
            });
        } else {
            // Skip EEP marker files — these define folder types, not entities
            if name == "_instance.toml" || name == "_service.toml" {
                continue;
            }

            let Some(file_type) = FileType::from_path(&path) else { continue };
            // Size from the source (Fjall has no mtime; modified is
            // only consulted by the disk file-watcher, which is moot
            // for Fjall-sourced loads). Use byte length for size.
            let size = source.read(&rel).map(|b| b.len() as u64).unwrap_or(0);
            let name = path.file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string();
            entries.push(FileMetadata {
                path,
                file_type,
                service: service.to_string(),
                name,
                size,
                modified: std::time::SystemTime::UNIX_EPOCH,
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
pub fn scan_space_directory(
    source: &dyn super::space_source::SpaceSource,
    space_root: &Path,
) -> Vec<FileMetadata> {
    let mut entries = Vec::new();

    // EEP-compliant service discovery, via the active source (Disk or
    // Fjall) — no `std::fs` so a migrated world reconstructs the full
    // service tree straight from the DB.
    let services = discover_services(source);

    for service_name in &services {
        if !source.exists(service_name) {
            continue;
        }
        let service_path = space_root.join(service_name);
        let children = scan_dir_entries(source, space_root, service_name, service_name);
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
/// Well-known service directory names — auto-discovered even without _service.toml marker.
const KNOWN_SERVICE_NAMES: &[&str] = &[
    "Workspace", "Lighting", "StarterGui", "SoulService", "MaterialService",
    "AdornmentService",
    "Players", "StarterPack", "StarterPlayer", "ReplicatedStorage",
    "ServerStorage", "ServerScriptService", "SoundService", "Teams", "Chat",
];

fn discover_services(source: &dyn super::space_source::SpaceSource) -> Vec<String> {
    let mut services = Vec::new();

    let entries = match source.list("") {
        Ok(entries) => entries,
        Err(e) => {
            warn!("Failed to list Space root via source: {}", e);
            return services;
        }
    };

    for ent in entries {
        if !ent.is_dir { continue; }
        let name = ent.name.as_str();

        // Discover via _service.toml marker OR well-known service name
        let service_marker = format!("{}/_service.toml", ent.rel_path);
        if source.exists(&service_marker) || KNOWN_SERVICE_NAMES.contains(&name) {
            services.push(name.to_string());
            debug!("Discovered service: {}", name);
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
    _meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    registry: &mut ResMut<SpaceFileRegistry>,
    material_registry: &mut ResMut<super::material_loader::MaterialRegistry>,
    mesh_cache: &mut ResMut<super::instance_loader::PrimitiveMeshCache>,
    space_path: &Path,
    file_meta: &FileMetadata,
    parent_entity: Option<Entity>,
    class_defaults: Option<&super::class_defaults::ClassDefaultsRegistry>,
    source: &dyn super::space_source::SpaceSource,
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
                // Load as service entity — sourced through SpaceSource
                // (Fjall when migrated) then parsed in memory.
                match src_read_string(source, space_path, &file_meta.path)
                    .map_err(|e| e.to_string())
                    .and_then(|c| super::service_loader::load_service_definition_from_str(&c))
                {
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
                // Load as instance entity — sourced through SpaceSource
                // (Fjall when migrated, Disk otherwise) then healed/
                // parsed in memory. This is the flat-file twin of the
                // Part-folder branch: no `std::fs`, and crucially no
                // schema self-heal write-back (the in-memory
                // `*_from_str` path never rewrites the file), so a
                // migrated/DB-authoritative world spawns instances with
                // zero disk reads and zero loose-file resurrection.
                let _ = class_defaults; // schema is the common-crate source of truth now
                match src_read_string(source, space_path, &file_meta.path)
                    .map_err(|e| e.to_string())
                    .and_then(|c| super::instance_loader::load_instance_definition_from_str(&c))
                {
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
                uuid: String::new(),
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
            match src_read_string(source, space_path, &file_meta.path) {
                Ok(markdown_source) => {
                    let e = commands.spawn((
                        eustress_common::classes::Instance {
                            name: file_meta.name.clone(),
                            class_name: eustress_common::classes::ClassName::SoulScript,
                            archivable: true,
                            id: 0,
                            ai: false,
                uuid: String::new(),
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
            match src_read_string(source, space_path, &file_meta.path) {
                Ok(rune_source) => {
                    let e = commands.spawn((
                        eustress_common::classes::Instance {
                            name: file_meta.name.clone(),
                            class_name: eustress_common::classes::ClassName::SoulScript,
                            archivable: true,
                            id: 0,
                            ai: false,
                uuid: String::new(),
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

        FileType::Lua => {
            // Luau/Lua scripts go through the mlua runtime. Spawned with the
            // same SoulScriptData component as Rune so `compile_scripts_on_play`
            // picks them up alongside .rune files; `run_context = Luau` routes
            // execution through `execute_chunk` instead of the Rune VM.
            match src_read_string(source, space_path, &file_meta.path) {
                Ok(lua_source) => {
                    let e = commands.spawn((
                        eustress_common::classes::Instance {
                            name: file_meta.name.clone(),
                            class_name: eustress_common::classes::ClassName::SoulScript,
                            archivable: true,
                            id: 0,
                            ai: false,
                            uuid: String::new(),
                        },
                        crate::soul::SoulScriptData {
                            source: lua_source,
                            dirty: false,
                            ast: None,
                            generated_code: None,
                            build_status: crate::soul::SoulBuildStatus::NotBuilt,
                            errors: Vec::new(),
                            run_context: crate::soul::SoulRunContext::Luau,
                        },
                        LoadedFromFile {
                            path: file_meta.path.clone(),
                            file_type: file_meta.file_type,
                            service: file_meta.service.clone(),
                        },
                        Name::new(file_meta.name.clone()),
                    )).id();
                    registry.register(file_meta.path.clone(), e, file_meta.clone());
                    info!("🌙 Loaded Luau script {} from {:?}", file_meta.name, file_meta.path);
                    e
                }
                Err(err) => {
                    error!("❌ Failed to read Luau script {:?}: {}", file_meta.path, err);
                    return None;
                }
            }
        }

        FileType::GuiElement => {
            // Load GUI element files (.textlabel.toml, .frame.toml, etc.) as Bevy UI entities.
            // Parses the TOML file for visual properties (position, size, colors, text)
            // and spawns with proper Bevy UI components (Node, BackgroundColor, Text, etc.)
            // so they render visually in the viewport.
            //
            // Sourced through SpaceSource (Fjall when migrated, Disk
            // otherwise) then parsed in memory — the flat-file twin of
            // the directory GUI branches, so a DB-authoritative world
            // loads GUI elements with zero disk reads.
            match src_read_string(source, space_path, &file_meta.path)
                .map_err(|e| e.to_string())
                .and_then(|c| super::gui_loader::load_gui_definition_from_str(&c))
            {
                Ok(gui_def) => {
                    let display_name = super::gui_loader::gui_display_name(&file_meta.path);
                    let gui_type = super::gui_loader::gui_class_from_extension(&file_meta.path);
                    let e = super::gui_loader::spawn_gui_element(
                        commands,
                        &file_meta.path,
                        &gui_def,
                    );
                    registry.register(file_meta.path.clone(), e, file_meta.clone());
                    // DEBUG: per-GUI-element; bulk-load hot path at scale.
                    debug!("🖼️ Loaded GUI element {} ({}) from {:?}", display_name, gui_type, file_meta.path);
                    e
                }
                Err(err) => {
                    error!("Failed to load GUI element {:?}: {}", file_meta.path, err);
                    return None;
                }
            }
        }

        FileType::Material => {
            // Load .mat.toml via SpaceSource (Fjall when migrated) then
            // parse in memory — zero disk on an authoritative world.
            match src_read_string(source, space_path, &file_meta.path)
                .map_err(|e| e.to_string())
                .and_then(|c| super::material_loader::load_material_definition_from_str(&c))
            {
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
                    commands.entity(e).insert(LoadedFromFile {
                        path: file_meta.path.clone(),
                        file_type: file_meta.file_type,
                        service: file_meta.service.clone(),
                    });
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

        FileType::Png => {
            // Spawn Image class entity for PNG files (textures in MaterialService)
            let img_name = file_meta.path.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Image".to_string());
            commands.spawn((
                eustress_common::classes::Instance {
                    name: img_name.clone(),
                    class_name: eustress_common::classes::ClassName::Image,
                    archivable: true,
                    id: 0,
                    ..Default::default()
                },
                LoadedFromFile {
                    path: file_meta.path.clone(),
                    file_type: file_meta.file_type,
                    service: file_meta.service.clone(),
                },
                Name::new(img_name),
            )).id()
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

    // Non-GUI, non-script files (sounds, loose data) inside StarterGui get a
    // hidden Node so Bevy doesn't treat them as stray non-UI leaves of the UI
    // root. Scripts are exempt — they carry SoulScriptData, not visuals, and
    // forcing them into the UI tree risks the UI layout system touching them
    // during play (historically scripts in StarterGui didn't execute).
    let is_script = matches!(file_meta.file_type, FileType::Soul | FileType::Rune | FileType::Lua);
    if file_meta.service == "StarterGui"
        && !matches!(file_meta.file_type, FileType::GuiElement)
        && !is_script
    {
        commands.entity(entity).insert(Node { display: Display::None, ..default() });
    }
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
    source: &dyn super::space_source::SpaceSource,
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

    // Check for service directory — either has _service.toml or is a well-known service name
    let service_toml_path = dir_meta.path.join("_service.toml");
    let service_toml_rel =
        super::space_source::rel_from_root(space_path, &service_toml_path).unwrap_or_default();
    let has_service_toml = source.exists(&service_toml_rel);
    let is_known_service = KNOWN_SERVICE_NAMES.contains(&dir_meta.name.as_str());
    if has_service_toml || is_known_service {
        // Load service definition from _service.toml, or create a default for known services
        let service_def = if has_service_toml {
            match src_read_string(source, space_path, &service_toml_path)
                .map_err(|e| e.to_string())
                .and_then(|c| super::service_loader::load_service_definition_from_str(&c))
            {
                Ok(def) => def,
                Err(err) => {
                    error!("Failed to load service {:?}: {}", service_toml_path, err);
                    return;
                }
            }
        } else {
            // Create a default definition for well-known services without _service.toml
            super::service_loader::ServiceDefinition {
                service: super::service_loader::ServiceProperties {
                    class_name: dir_meta.name.clone(),
                    icon: None,
                    description: None,
                    can_have_children: true,
                    properties: std::collections::HashMap::new(),
                },
                metadata: super::service_loader::ServiceMetadata::default(),
                properties: std::collections::HashMap::new(),
            }
        };

        let is_gui_service = dir_meta.name == "StarterGui";
        let service_entity = if is_gui_service {
            // StarterGui must be a Bevy UI root so child ScreenGui/Frame entities render.
            // A Transform+Visibility parent breaks the Bevy UI hierarchy chain.
            super::service_loader::spawn_service_as_ui_root(commands, service_toml_path.clone(), service_def)
        } else {
            super::service_loader::spawn_service(commands, service_toml_path.clone(), service_def)
        };
        registry.register(dir_meta.path.clone(), service_entity, dir_meta.clone());
        info!("🏢 Spawned Service '{}' with {} children", dir_meta.name, dir_meta.children.len());

        // Spawn all children parented to this service
        for child in &dir_meta.children {
            match child.file_type {
                FileType::Directory => {
                    spawn_directory_entry(
                        commands, asset_server, meshes, materials, registry,
                        material_registry, mesh_cache, space_path, child, Some(service_entity),
                        class_defaults, source,
                    );
                }
                _ => {
                    spawn_file_entry(
                        commands, asset_server, meshes, materials, registry,
                        material_registry, mesh_cache, space_path, child, Some(service_entity),
                        class_defaults, source,
                    );
                }
            }
        }
        return;
    }

    // Check for _instance.toml — it may declare a richer class
    // (ScreenGui, Frame, BillboardGui, TextLabel, …). Historic on-disk
    // files are `[metadata] class_name` snake_case; files left over
    // from the aborted PascalCase migration are `[Metadata] ClassName`.
    // Try both so either layout keeps loading. Class-name dispatch
    // delegates to `ClassName::from_str` — the canonical string→enum
    // map lives on the enum itself, so adding a new class means one
    // variant + one from_str arm in `common/classes.rs`, no changes
    // here. Legacy `"Script"` alias routes to `SoulScript`.
    let instance_toml_path = dir_meta.path.join("_instance.toml");
    let instance_toml_rel = super::space_source::rel_from_root(space_path, &instance_toml_path)
        .unwrap_or_default();
    let class_name = if source.exists(&instance_toml_rel) {
        src_read_string(source, space_path, &instance_toml_path)
            .ok()
            .and_then(|s| toml::from_str::<toml::Value>(&s).ok())
            .and_then(|v| {
                let meta = v.get("metadata").or_else(|| v.get("Metadata"))?;
                let cn = meta.get("class_name").or_else(|| meta.get("ClassName"))?;
                cn.as_str().map(|s| s.to_string())
            })
            .map(|cn| {
                // Legacy shim: "Script" used to mean the Rune script
                // class before the Soul/Luau split.
                let cn_resolved = if cn == "Script" { "SoulScript" } else { cn.as_str() };
                eustress_common::classes::ClassName::from_str(cn_resolved)
                    .unwrap_or(eustress_common::classes::ClassName::Folder)
            })
            .unwrap_or(eustress_common::classes::ClassName::Folder)
    } else {
        eustress_common::classes::ClassName::Folder
    };

    // Spawn the Folder / ScreenGui / Frame / Model entity
    let is_screen_gui = matches!(class_name, eustress_common::classes::ClassName::ScreenGui);
    let is_gui_container = matches!(class_name,
        eustress_common::classes::ClassName::Frame
        | eustress_common::classes::ClassName::ScrollingFrame
    );

    let folder_entity = if is_screen_gui {
        // ScreenGui: fullscreen UI root — read enabled/visible from _instance.toml
        let instance_toml = dir_meta.path.join("_instance.toml");
        let screen_gui_visible = if let Ok(gui_def) = src_read_string(source, space_path, &instance_toml).map_err(|e| e.to_string()).and_then(|c| super::gui_loader::load_gui_definition_from_str(&c)) {
            gui_def.gui.visible
        } else {
            true // default: visible
        };

        let entity_cmds = commands.spawn((
            eustress_common::classes::Instance {
                name: dir_meta.name.clone(),
                class_name,
                archivable: true,
                id: 0,
                ai: false,
                uuid: String::new(),
            },
            LoadedFromFile {
                path: dir_meta.path.clone(),
                file_type: FileType::Directory,
                service: dir_meta.service.clone(),
            },
            Name::new(dir_meta.name.clone()),
            Node { display: Display::None, ..default() },
            // GuiElementDisplay with visible flag — children inherit this for rendering
            eustress_common::gui::billboard_renderer::GuiElementDisplay {
                x: 0.0, y: 0.0, width: 0.0, height: 0.0,
                position_udim2: [0.0; 4], size_udim2: [0.0; 4],
                anchor_point: [0.0, 0.0],
                z_order: 0, visible: screen_gui_visible, clip_children: false,
                scroll_x: 0.0, scroll_y: 0.0,
                bg_color: [0.0; 4], border_size: 0.0, border_color: [0.0; 4],
                corner_radius: 0.0,
                text: String::new(), text_color: [1.0; 4],
                font_size: 14.0, font_weight: 400,
                text_align: "Center".to_string(), text_y_align: "Center".to_string(),
                text_stroke_color: [0.0, 0.0, 0.0, 0.0],
                text_scaled: false,
                image_path: String::new(),
                class_type: "screengui".to_string(),
                mouse_filter: "ignore".to_string(),
            },
        ));
        if !screen_gui_visible {
            info!("📋 ScreenGui '{}' loaded as hidden (enabled=false)", dir_meta.name);
        }
        entity_cmds.id()
    } else if is_gui_container {
        // Frame/ScrollingFrame directory — load GUI properties from _instance.toml
        // and attach GuiElementDisplay so it renders through Slint overlay
        let instance_toml = dir_meta.path.join("_instance.toml");
        let gui_display = if let Ok(gui_def) = src_read_string(source, space_path, &instance_toml).map_err(|e| e.to_string()).and_then(|c| super::gui_loader::load_gui_definition_from_str(&c)) {
            let class_str = format!("{:?}", class_name).to_lowercase();
            super::gui_loader::gui_display_from_props(&gui_def.gui, gui_def.text.as_ref(), &class_str)
        } else {
            // Fallback: invisible container
            eustress_common::gui::billboard_renderer::GuiElementDisplay {
                x: 0.0, y: 0.0, width: 0.0, height: 0.0,
                position_udim2: [0.0; 4], size_udim2: [0.0; 4],
                anchor_point: [0.0, 0.0],
                z_order: 1, visible: true, clip_children: false,
                scroll_x: 0.0, scroll_y: 0.0,
                bg_color: [0.0, 0.0, 0.0, 0.0],
                border_size: 0.0, border_color: [0.0; 4],
                corner_radius: 0.0,
                text: String::new(), text_color: [1.0; 4],
                font_size: 14.0, font_weight: 400,
                text_align: "Center".to_string(), text_y_align: "Center".to_string(),
                text_stroke_color: [0.0, 0.0, 0.0, 0.0],
                text_scaled: false,
                image_path: String::new(),
                class_type: "Frame".to_string(),
                mouse_filter: "stop".to_string(),
            }
        };
        commands.spawn((
            eustress_common::classes::Instance {
                name: dir_meta.name.clone(),
                class_name,
                archivable: true,
                id: 0,
                ai: false,
                uuid: String::new(),
            },
            LoadedFromFile {
                path: dir_meta.path.clone(),
                file_type: FileType::Directory,
                service: dir_meta.service.clone(),
            },
            Name::new(dir_meta.name.clone()),
            Node { display: Display::None, ..default() },
            gui_display,
        )).id()
    } else if matches!(class_name, eustress_common::classes::ClassName::BillboardGui) {
        // BillboardGui — 3D billboard entity (quad facing camera).
        //
        // Roblox parity: every `BillboardGui` instance property is
        // round-tripped through `_instance.toml`. Optional fields default
        // to the class's `Default::default()` so older TOML files (which
        // pre-date a property) load cleanly without overwriting existing
        // defaults.
        let instance_toml = dir_meta.path.join("_instance.toml");
        let mut bb_class = eustress_common::classes::BillboardGui::default();
        let mut bb_offset = Vec3::new(0.0, 2.0, 0.0);
        // Collected from `gui_def.tags` so we can attach the ECS Tags
        // component after spawn (lives outside the `if let Ok` scope).
        let mut bb_tags: Vec<String> = Vec::new();

        if let Ok(gui_def) = src_read_string(source, space_path, &instance_toml).map_err(|e| e.to_string()).and_then(|c| super::gui_loader::load_gui_definition_from_str(&c)) {
            bb_tags = gui_def.tags.clone();
            let g = &gui_def.gui;

            // Geometry — strict UDim2 from the schema, no legacy fallback.
            bb_class.size = g.size;
            if let Some(v) = g.size_offset { bb_class.size_offset = v; }
            if let Some(v) = g.extents_offset { bb_class.extents_offset = v; }
            if let Some(v) = g.extents_offset_world_space { bb_class.extents_offset_world_space = v; }
            if let Some(v) = g.units_offset { bb_class.units_offset = v; }
            if let Some(v) = g.units_offset_world_space { bb_class.units_offset_world_space = v; }

            // BillboardGui's 3D placement still comes from `units_offset`
            // (Vec3 stud offset) — separate from the 2D-canvas
            // `position: UDim2`. Mirror it into `bb_offset` for the
            // entity's Transform.
            bb_offset = Vec3::new(
                bb_class.units_offset[0],
                bb_class.units_offset[1],
                bb_class.units_offset[2],
            );

            // Distance
            if let Some(v) = g.max_distance { bb_class.max_distance = v; }
            if let Some(v) = g.distance_lower_limit { bb_class.distance_lower_limit = v; }
            if let Some(v) = g.distance_upper_limit { bb_class.distance_upper_limit = v; }
            if let Some(v) = g.distance_step { bb_class.distance_step = v; }

            // Behaviour flags
            if let Some(v) = g.active { bb_class.active = v; }
            if let Some(v) = g.enabled { bb_class.enabled = v; }
            if let Some(v) = g.always_on_top { bb_class.always_on_top = v; }
            if let Some(v) = g.clips_descendants { bb_class.clips_descendants = v; }
            if let Some(v) = g.reset_on_spawn { bb_class.reset_on_spawn = v; }
            if let Some(v) = g.stiffness_by_distance { bb_class.stiffness_by_distance = v; }

            // Appearance
            if let Some(v) = g.brightness { bb_class.brightness = v; }
            if let Some(v) = g.light_influence { bb_class.light_influence = v; }

            // Sorting
            if let Some(ref s) = g.z_index_behavior {
                bb_class.z_index_behavior = match s.as_str() {
                    "Global" => eustress_common::classes::ZIndexBehavior::Global,
                    _ => eustress_common::classes::ZIndexBehavior::Sibling,
                };
            }
            // NOTE: `g.adornee` carries the instance name; resolution to
            // an Entity reference happens in a post-load pass once all
            // instances are spawned (see resolve_billboard_adornees, TODO).
        }

        // Build the runtime marker by copying the parity-relevant subset
        // of the class. `sync_billboard_class_to_marker` will keep these
        // in sync on subsequent edits, but we initialise them here so
        // the first frame shows correct visibility / depth / size.
        // Marker mirrors the renderer's resolved-pixel inputs. Class
        // carries the source-of-truth UDim2; we resolve to pixels here
        // (Scale × PIXELS_PER_METER + Offset) so the renderer doesn't
        // have to revisit UDim2 math each frame.
        let [size_w_px, size_h_px] = bb_class.size.to_pixels(50.0, 50.0);
        // `size_offset` is Roblox-parity `Vector2` — already in pixels.
        let [size_off_x, size_off_y] = bb_class.size_offset;
        let marker = eustress_common::gui::billboard_renderer::BillboardGuiMarker {
            size: [size_w_px.max(1.0), size_h_px.max(1.0)],
            size_offset: [size_off_x, size_off_y],
            extents_offset: bb_class.extents_offset,
            extents_offset_world_space: bb_class.extents_offset_world_space,
            units_offset_world_space: bb_class.units_offset_world_space,
            max_distance: if bb_class.max_distance > 0.0 && bb_class.distance_upper_limit > 0.0 {
                bb_class.max_distance.min(bb_class.distance_upper_limit)
            } else {
                bb_class.max_distance.max(bb_class.distance_upper_limit)
            },
            distance_lower_limit: bb_class.distance_lower_limit.max(0.0),
            distance_step: bb_class.distance_step.max(0.0),
            always_on_top: bb_class.always_on_top,
            clips_descendants: bb_class.clips_descendants,
            brightness: bb_class.brightness.clamp(0.0, 8.0),
            light_influence: bb_class.light_influence.clamp(0.0, 1.0),
            face_camera: true,
            visible: bb_class.enabled,
            z_index: bb_class.z_index,
        };

        // The Properties panel keys its rich-class display branch off
        // `InstanceFile` (it's the marker that says "this entity has a
        // canonical `_instance.toml` on disk you can edit"). Without it,
        // a freshly-reloaded BillboardGui falls through to the basic
        // Data + Transform fallback and the user can't edit AlwaysOnTop /
        // MaxDistance / Brightness / etc. through the panel.
        let instance_toml_path = dir_meta.path.join("_instance.toml");
        let entity = commands.spawn((
            eustress_common::classes::Instance {
                name: dir_meta.name.clone(),
                class_name,
                archivable: true,
                id: 0,
                ai: false,
                uuid: String::new(),
            },
            bb_class,
            marker,
            LoadedFromFile {
                path: dir_meta.path.clone(),
                file_type: FileType::Directory,
                service: dir_meta.service.clone(),
            },
            super::instance_loader::InstanceFile {
                toml_path: instance_toml_path,
                mesh_path: std::path::PathBuf::new(),
                name: dir_meta.name.clone(),
            },
            Name::new(dir_meta.name.clone()),
            Transform::from_translation(bb_offset),
            Visibility::default(),
        )).id();
        // Tag hydration matches `instance_loader::spawn_instance`'s path
        // for Parts so MCP / ECS queries see GUI tags identically.
        if !bb_tags.is_empty() {
            commands.entity(entity).insert(eustress_common::attributes::Tags(bb_tags));
        }
        entity
    } else if matches!(class_name, eustress_common::classes::ClassName::Image | eustress_common::classes::ClassName::Video) {
        // Image / Video — imported media class. Loads the asset_path
        // referenced by `[asset].path` in _instance.toml, which lives
        // under the Universe-level `assets/` folder. The entity is a
        // 3D quad: for Image, the texture is the loaded image; for
        // Video, a mid-grey placeholder material until the decoder
        // integration lands.
        let instance_toml = dir_meta.path.join("_instance.toml");
        let toml_value: Option<toml::Value> =
            src_read_string(source, space_path, &instance_toml)
                .ok()
                .and_then(|s| toml::from_str(&s).ok());

        // Universe-relative asset path stored in `[asset].path`. The
        // engine-runtime path is `<Universe>/<asset_path>`.
        let asset_rel = toml_value
            .as_ref()
            .and_then(|v| v.get("asset"))
            .and_then(|a| a.get("path"))
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string();

        // Per-class section: [image] for Image, [video] for Video.
        let section_name = match class_name {
            eustress_common::classes::ClassName::Image => "image",
            _                                          => "video",
        };
        let section = toml_value.as_ref().and_then(|v| v.get(section_name));
        let size_xy = section
            .and_then(|s| s.get("size"))
            .and_then(|s| s.as_array())
            .and_then(|arr| {
                let x = arr.first()?.as_float().map(|f| f as f32)?;
                let y = arr.get(1)?.as_float().map(|f| f as f32)?;
                Some([x, y])
            })
            .unwrap_or(match class_name {
                eustress_common::classes::ClassName::Image => [4.0, 4.0],
                _                                          => [6.0, 3.375],
            });
        let color_rgba = section
            .and_then(|s| s.get("color"))
            .and_then(|c| c.as_array())
            .and_then(|arr| {
                Some([
                    arr.first()?.as_float()? as f32,
                    arr.get(1)?.as_float()? as f32,
                    arr.get(2)?.as_float()? as f32,
                    arr.get(3).and_then(|v| v.as_float()).unwrap_or(1.0) as f32,
                ])
            })
            .unwrap_or([1.0, 1.0, 1.0, 1.0]);
        let transparency = section
            .and_then(|s| s.get("transparency"))
            .and_then(|t| t.as_float())
            .map(|f| f as f32)
            .unwrap_or(0.0);
        let position = toml_value
            .as_ref()
            .and_then(|v| v.get("transform"))
            .and_then(|t| t.get("position"))
            .and_then(|p| p.as_array())
            .and_then(|arr| {
                Some(Vec3::new(
                    arr.first()?.as_float()? as f32,
                    arr.get(1)?.as_float()? as f32,
                    arr.get(2)?.as_float()? as f32,
                ))
            })
            .unwrap_or(Vec3::new(0.0, 2.0, 0.0));

        // Resolve the asset on disk. Universe root = grandparent of
        // SpaceRoot (`<Universe>/Spaces/<SpaceN>`); asset path is
        // joined from there.
        let universe_root = space_path
            .ancestors()
            .nth(2)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| space_path.to_path_buf());
        let abs_asset_path = universe_root.join(&asset_rel);

        // Build a unit quad mesh — same shape as `BillboardCard`'s
        // quad, scaled to size via Transform.scale.
        let mesh_handle = meshes.add(build_imported_media_quad());

        // Material: Image gets the loaded texture as albedo; Video
        // gets a mid-grey placeholder material. Both honour the part's
        // colour tint + transparency.
        let alpha = 1.0 - transparency.clamp(0.0, 1.0);
        let mut mat = StandardMaterial {
            base_color: bevy::color::Color::srgba(color_rgba[0], color_rgba[1], color_rgba[2], color_rgba[3] * alpha),
            unlit: true,
            alpha_mode: if alpha < 1.0 || matches!(class_name, eustress_common::classes::ClassName::Image) {
                bevy::prelude::AlphaMode::Blend
            } else {
                bevy::prelude::AlphaMode::Opaque
            },
            cull_mode: None,
            ..default()
        };
        if matches!(class_name, eustress_common::classes::ClassName::Image) {
            // Bevy's AssetServer wants paths relative to its asset root.
            // The simplest cross-platform approach: pass the absolute
            // path as a string. AssetServer accepts absolute filesystem
            // paths on the filesystem source.
            if !asset_rel.is_empty() && abs_asset_path.exists() {
                let texture: Handle<bevy::image::Image> = asset_server.load(abs_asset_path.clone());
                mat.base_color_texture = Some(texture);
            } else {
                warn!(
                    "🖼️ Image entity '{}' has missing asset at {:?} — rendering as flat colour quad",
                    dir_meta.name, abs_asset_path
                );
            }
        } else {
            // Video — initial material is placeholder (replaced next
            // frame by `attach_pending_video_players`, which binds the
            // VideoPlayer's GPU image as base_color_texture). The
            // placeholder colour shows for ~1 frame between spawn and
            // pump kickoff; imperceptible.
            mat.base_color = bevy::color::Color::srgba(0.18, 0.20, 0.22, alpha);
            mat.emissive = bevy::color::LinearRgba::new(0.05, 0.06, 0.08, 1.0);
        }
        let material_handle = materials.add(mat);

        // Build the entity. For Video, also attach `PendingVideoSetup`
        // so the video plugin's deferred-attach system mints a real
        // VideoPlayer next frame and rewrites the material's base
        // color texture to point at the freshly-decoded frames.
        let is_video = matches!(class_name, eustress_common::classes::ClassName::Video);
        let entity = commands.spawn((
            eustress_common::classes::Instance {
                name: dir_meta.name.clone(),
                class_name,
                archivable: true,
                id: 0,
                ai: false,
                uuid: String::new(),
            },
            Mesh3d(mesh_handle),
            MeshMaterial3d(material_handle),
            LoadedFromFile {
                path: dir_meta.path.clone(),
                file_type: FileType::Directory,
                service: dir_meta.service.clone(),
            },
            Name::new(dir_meta.name.clone()),
            Transform::from_translation(position).with_scale(Vec3::new(size_xy[0], size_xy[1], 1.0)),
            Visibility::default(),
            bevy::light::NotShadowCaster,
        )).id();

        if is_video {
            // Texture-buffer dimensions for the video. 1280×720 gives a
            // sensible default that handles most imported clips without
            // being wasteful for thumbnails. Real decoder integration
            // will set width/height from the file's video stream
            // metadata when the asset opens.
            let video_w: u32 = 1280;
            let video_h: u32 = 720;
            commands.entity(entity).insert(crate::video::PendingVideoSetup {
                width: video_w,
                height: video_h,
                asset_path: abs_asset_path.clone(),
            });
        }

        entity
    } else if matches!(class_name, eustress_common::classes::ClassName::SoulScript) {
        // Script folder — find the .rune/.luau/.soul source file inside and load it.
        let instance_toml = dir_meta.path.join("_instance.toml");
        // Read the "source" field from _instance.toml to find the script filename,
        // or scan the folder for the first .rune/.luau/.soul file.
        let source_file = src_read_string(source, space_path, &instance_toml)
            .ok()
            .and_then(|s| toml::from_str::<toml::Value>(&s).ok())
            .and_then(|v| {
                use eustress_common::class_schema::get_section_insensitive as get_ci;
                get_ci(&v, "script")
                    .and_then(|s| get_ci(s, "source"))
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string())
            })
            .map(|rel| dir_meta.path.join(rel))
            .or_else(|| {
                // Fallback: scan the folder via the source for the first
                // script file. `dir_meta.path` is absolute; derive the
                // Space-relative dir so Fjall-sourced worlds resolve it.
                let dir_rel = super::space_source::rel_from_root(space_path, &dir_meta.path)
                    .unwrap_or_default();
                source.list(&dir_rel).ok().and_then(|entries| {
                    entries
                        .into_iter()
                        .find(|e| {
                            !e.is_dir
                                && (e.name.ends_with(".rune")
                                    || e.name.ends_with(".luau")
                                    || e.name.ends_with(".soul")
                                    || e.name.ends_with(".lua"))
                        })
                        .map(|e| {
                            let mut p = space_path.to_path_buf();
                            for seg in e.rel_path.split('/') {
                                if !seg.is_empty() {
                                    p.push(seg);
                                }
                            }
                            p
                        })
                })
            });

        if let Some(ref src_path) = source_file {
            if let Ok(script_src) = src_read_string(source, space_path, src_path) {
                let script_name = dir_meta.name.clone();
                commands.spawn((
                    eustress_common::classes::Instance {
                        name: script_name.clone(),
                        class_name: eustress_common::classes::ClassName::SoulScript,
                        archivable: true, id: 0, ai: false, uuid: String::new(),
                    },
                    crate::soul::SoulScriptData {
                        source: script_src,
                        dirty: false,
                        ast: None,
                        generated_code: None,
                        build_status: crate::soul::SoulBuildStatus::NotBuilt,
                        errors: Vec::new(),
                        run_context: Default::default(),
                    },
                    LoadedFromFile {
                        path: dir_meta.path.clone(),
                        file_type: FileType::Directory,
                        service: dir_meta.service.clone(),
                    },
                    Name::new(script_name),
                )).id()
            } else {
                warn!("Failed to read script source {:?}", src_path);
                commands.spawn((
                    eustress_common::classes::Instance {
                        name: dir_meta.name.clone(),
                        class_name: eustress_common::classes::ClassName::Folder,
                        archivable: true, id: 0, ai: false, uuid: String::new(),
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
            }
        } else {
            warn!("Script folder {:?} has no source file", dir_meta.path);
            commands.spawn((
                eustress_common::classes::Instance {
                    name: dir_meta.name.clone(),
                    class_name: eustress_common::classes::ClassName::Folder,
                    archivable: true, id: 0, ai: false, uuid: String::new(),
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
        }
    } else if matches!(class_name, eustress_common::classes::ClassName::Part) {
        // Part folder — load via spawn_instance (same path as flat .glb.toml files).
        // The _instance.toml inside contains the full InstanceDefinition with mesh, transform, etc.
        // Realism sections (`[material]` / `[thermodynamic]` / `[electrochemical]`)
        // are dynamic on every Part — `InstanceDefinition` consumes them as
        // typed fields and `spawn_instance` attaches the matching ECS
        // components when present, with no subclass required.
        let instance_toml = dir_meta.path.join("_instance.toml");
        // Source the TOML through the active SpaceSource (Fjall when
        // migrated, Disk otherwise) then heal/parse in memory — no
        // `std::fs` so a Fjall-authoritative world spawns Parts with
        // zero disk reads. `instance_toml` (absolute) is still handed
        // to `spawn_instance` for InstanceFile identity / write-back.
        let parsed = src_read_string(source, space_path, &instance_toml)
            .map_err(|e| e.to_string())
            .and_then(|content| {
                super::instance_loader::load_instance_definition_from_str(&content)
            });
        match parsed {
            Ok(instance_def) => {
                // spawn_instance attaches InstanceFile internally with the toml_path
                super::instance_loader::spawn_instance(
                    commands,
                    asset_server,
                    materials,
                    material_registry,
                    mesh_cache,
                    instance_toml,
                    instance_def,
                )
            }
            Err(e) => {
                warn!("Failed to load Part folder {:?}: {}", dir_meta.path, e);
                // Fall back to empty folder entity
                commands.spawn((
                    eustress_common::classes::Instance {
                        name: dir_meta.name.clone(),
                        class_name: eustress_common::classes::ClassName::Folder,
                        archivable: true, id: 0, ai: false, uuid: String::new(),
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
            }
        }
    } else if matches!(class_name,
        eustress_common::classes::ClassName::TextLabel
        | eustress_common::classes::ClassName::TextButton
        | eustress_common::classes::ClassName::TextBox
        | eustress_common::classes::ClassName::ImageLabel
        | eustress_common::classes::ClassName::ImageButton
        | eustress_common::classes::ClassName::ViewportFrame,
    ) {
        // Leaf UI class in folder form — new Insert-menu convention
        // writes `Name/_instance.toml` with `class_name = "TextLabel"`
        // (etc.) instead of the legacy flat `Name.textlabel.toml`.
        // Route through the same `spawn_gui_element` helper the flat
        // path uses so Bevy UI components, click handlers, and text
        // rendering all stay in one implementation. The loader at
        // `gui_class_from_extension` now peeks the `_instance.toml`
        // metadata when the file ends in `_instance.toml`, so
        // `spawn_gui_element` resolves the right class without any
        // extra parameter threading here.
        let instance_toml = dir_meta.path.join("_instance.toml");
        match src_read_string(source, space_path, &instance_toml).map_err(|e| e.to_string()).and_then(|c| super::gui_loader::load_gui_definition_from_str(&c)) {
            Ok(gui_def) => {
                super::gui_loader::spawn_gui_element(commands, &instance_toml, &gui_def)
            }
            Err(e) => {
                warn!("Failed to load leaf UI folder {:?}: {}", dir_meta.path, e);
                commands.spawn((
                    eustress_common::classes::Instance {
                        name: dir_meta.name.clone(),
                        class_name: eustress_common::classes::ClassName::Folder,
                        archivable: true, id: 0, ai: false, uuid: String::new(),
                    },
                    LoadedFromFile {
                        path: dir_meta.path.clone(),
                        file_type: FileType::Directory,
                        service: dir_meta.service.clone(),
                    },
                    Name::new(dir_meta.name.clone()),
                    Node { display: Display::None, ..default() },
                )).id()
            }
        }
    } else {
        // Regular Folder / Model — 3D entity
        let display_name = {
            let mut chars = dir_meta.name.chars();
            match chars.next() {
                None => dir_meta.name.clone(),
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            }
        };
        commands.spawn((
            eustress_common::classes::Instance {
                name: display_name.clone(),
                class_name,
                archivable: true,
                id: 0,
                ai: false,
                uuid: String::new(),
            },
            LoadedFromFile {
                path: dir_meta.path.clone(),
                file_type: FileType::Directory,
                service: dir_meta.service.clone(),
            },
            Name::new(display_name),
            Transform::default(),
            Visibility::default(),
        )).id()
    };

    // Non-GUI directories (e.g. "scripts/") inside StarterGui get a hidden Node
    // so Bevy doesn't treat them as stray non-UI leaves of the UI root.
    // Script folders (SoulScript) are exempt — they host SoulScriptData and
    // shouldn't be dragged into the UI layout pass.
    let is_non_gui_dir = dir_meta.service == "StarterGui" && !is_screen_gui && !is_gui_container
        && !matches!(class_name,
            eustress_common::classes::ClassName::Frame
            | eustress_common::classes::ClassName::ScrollingFrame
            | eustress_common::classes::ClassName::BillboardGui
            | eustress_common::classes::ClassName::SurfaceGui
            | eustress_common::classes::ClassName::SoulScript
        );
    if is_non_gui_dir {
        // Spawned with Transform+Visibility above — swap to hidden Node
        commands.entity(folder_entity)
            .remove::<Transform>()
            .remove::<Visibility>()
            .insert(Node { display: Display::None, ..default() });
    }
    if let Some(parent) = parent_entity {
        commands.entity(folder_entity).insert(ChildOf(parent));
    }

    registry.register(dir_meta.path.clone(), folder_entity, dir_meta.clone());
    // Also index the entity under its `_instance.toml` marker when one
    // exists. The file watcher delivers Modify/Remove events against
    // the FILE (the marker), not the enclosing folder; without this
    // secondary entry, UPDATE / DELETE hot-reloads on an initially-
    // scanned Part folder couldn't resolve the entity and silently
    // no-op'd. Hot-created entities already register under the
    // `_instance.toml` path (see `handle_file_created` Toml branch),
    // so this just brings initial-scan registration into parity.
    let instance_marker = dir_meta.path.join("_instance.toml");
    if instance_marker.is_file() {
        registry.register(instance_marker, folder_entity, dir_meta.clone());
    }
    // DEBUG, not INFO: this fires once per directory ENTITY. At 50k
    // (the benchmark) an INFO here is 50k synchronous stderr writes
    // under a lock — ~3-4ms each ≈ minutes of pure logging that
    // throttles the entire load (a log-I/O stall, the same class of
    // bug as the old 53s write storm). Keep it for single-entity
    // debugging only; never on the bulk-load hot path.
    debug!("📁 Spawned Folder '{}' ({} items)", dir_meta.name, dir_meta.children.len());

    // SoulScript folders are full leaves — their children are source +
    // build artefacts (`.rune`, `Summary.md`), never scene entities.
    let is_full_leaf = matches!(class_name, eustress_common::classes::ClassName::SoulScript);
    if is_full_leaf {
        return;
    }

    // Part folders are *partial* leaves: their FILE children are internal
    // assets (`.glb`, `.toml` mesh metadata) that we don't want to surface
    // as scene entities, but their DIRECTORY children CAN be UI hangers-on
    // (BillboardGui labels, SurfaceGui overlays) that the user attached
    // via MindSpace and absolutely must reload across sessions. Without
    // this descent, MindSpace-created billboards survived only one
    // session — the TOML on disk was correct but the loader never saw it.
    let part_subfolders_only = matches!(class_name, eustress_common::classes::ClassName::Part);

    // Spawn all children parented to this folder, frame-budgeted.
    for (idx, child) in dir_meta.children.iter().enumerate() {
        // Once the per-frame budget is spent, spill the REMAINING
        // children (each carries this just-spawned folder as its
        // parent, so linkage is preserved) to the streaming queue and
        // stop. `drain_pending_spawns` continues them over subsequent
        // frames — a 50k subtree never blocks a single frame again.
        // `fetch_sub` returns the value BEFORE decrement, so exactly
        // `budget` children are processed before the first spill; with
        // the `i64::MAX` default (unbudgeted paths) this never trips.
        if SPAWN_BUDGET.fetch_sub(1, std::sync::atomic::Ordering::Relaxed) <= 0 {
            // This subtree is bigger than a whole frame's budget — by
            // definition a dense scene. Engage adaptive material-color
            // quantization so its (potentially all-unique) colors
            // collapse into a few batched GPU materials instead of one
            // draw call per part. No-op for any scene that never spills.
            super::material_loader::set_dense_material_mode(true);
            let mut q = SPILL.lock().unwrap_or_else(|e| e.into_inner());
            for rest in &dir_meta.children[idx..] {
                q.push((rest.clone(), Some(folder_entity)));
            }
            break;
        }
        match child.file_type {
            FileType::Directory => {
                spawn_directory_entry(
                    commands, asset_server, meshes, materials, registry,
                    material_registry, mesh_cache, space_path, child, Some(folder_entity),
                    class_defaults, source,
                );
            }
            _ => {
                if part_subfolders_only {
                    // Skip raw files inside a Part folder (`.glb`, mesh
                    // TOML metadata) — those aren't scene entities.
                    continue;
                }
                spawn_file_entry(
                    commands, asset_server, meshes, materials, registry,
                    material_registry, mesh_cache, space_path, child, Some(folder_entity),
                    class_defaults, source,
                );
            }
        }
    }
}

/// Per-frame cap on synchronous entity spawns during Space load.
///
/// A service subtree with this many nodes or fewer loads FULLY in one
/// frame, byte-for-byte as before (normal scenes / the MegaTower city
/// are unchanged — zero regression). Only a larger subtree (the 50k
/// benchmark) spills its overflow to [`SPILL`] and streams over later
/// frames via [`drain_pending_spawns`], eliminating the multi-second
/// single-frame freeze that was the verified 4-FPS root cause
/// (`Workspace ∈ PRIORITY_SERVICES` → entire subtree recursed
/// synchronously in one frame).
const SPAWN_BUDGET_PER_FRAME: i64 = 4096;

/// Remaining spawn budget for the current frame. `i64::MAX` ==
/// "unbudgeted": any spawn path NOT driven by the budgeted loader
/// (hot-reload, single-entity create, MCP) is completely unaffected.
/// The budgeted systems store a finite value before recursing.
static SPAWN_BUDGET: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(i64::MAX);

/// Overflow queue of `(node, parent)` pairs deferred past the frame
/// budget. `Vec` (not `VecDeque`) so the `static` initializer uses the
/// long-stable const `Vec::new()`; drained as a front batch per frame.
static SPILL: std::sync::Mutex<Vec<(FileMetadata, Option<Entity>)>> =
    std::sync::Mutex::new(Vec::new());

/// `SpaceLoadGeneration` the current [`SPILL`] belongs to. A genuine
/// Space switch bumps the generation and the drain discards stale
/// spill instead of spawning it into the wrong Space.
static SPILL_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Arm the per-frame spawn budget for a fresh FULL load — both the
/// initial `load_space_files_system` AND `apply_space_rescan` (the
/// rescan was a second un-budgeted full-50k path: it re-ran the whole
/// scan+spawn synchronously every time `SpaceRescanNeeded` fired, which
/// is the periodic multi-second stutter). Discards prior spill, tags
/// the generation, gives this frame a full budget.
pub(crate) fn begin_budgeted_load(generation: u64) {
    {
        let mut q = SPILL.lock().unwrap_or_else(|e| e.into_inner());
        q.clear();
    }
    SPILL_GEN.store(generation, std::sync::atomic::Ordering::Relaxed);
    SPAWN_BUDGET.store(SPAWN_BUDGET_PER_FRAME, std::sync::atomic::Ordering::Relaxed);
    // Every fresh load starts assuming a normal-sized scene: lossless
    // material keys (zero visual change). The first frame-budget spill
    // below re-engages dense color quantization for huge scenes only.
    super::material_loader::set_dense_material_mode(false);
}

/// Drop every queued spilled spawn. Called by the Space teardown
/// BEFORE it despawns the outgoing world's entities, so
/// `drain_pending_spawns` can't spawn an old world's children parented
/// to entities that are about to be destroyed (the dead-`ChildOf`
/// orphan storm). The next load's `begin_budgeted_load` re-arms a
/// fresh generation.
pub(crate) fn discard_pending_spawns() {
    let mut q = SPILL.lock().unwrap_or_else(|e| e.into_inner());
    let n = q.len();
    q.clear();
    if n > 0 {
        warn!(
            target: "eustress_engine::world_db",
            "🧹 Discarded {} queued spilled spawns on Space teardown (prevents dead-ChildOf orphan storm)",
            n
        );
    }
}

/// Re-arm the budget before each priority service so a small one
/// (`Lighting`) still loads instantly even after a huge one spilled.
pub(crate) fn rearm_priority_budget() {
    SPAWN_BUDGET.store(SPAWN_BUDGET_PER_FRAME, std::sync::atomic::Ordering::Relaxed);
}

/// Priority services loaded immediately at startup (the 3D scene).
/// Everything else is deferred to avoid blocking the first frame.
pub const PRIORITY_SERVICES: &[&str] = &["Workspace", "Lighting"];

/// Monotonically-incrementing counter bumped on every `open_space` call.
/// DeferredServiceLoader stamps the generation it was built for; if the
/// live counter moves on (rename, switch, or rapid re-open) the stale
/// queue is discarded before it can load entries into the wrong Space.
#[derive(Resource, Default)]
pub struct SpaceLoadGeneration(pub u64);

/// Resource tracking deferred service loading state
#[derive(Resource, Default)]
pub struct DeferredServiceLoader {
    /// Services waiting to be loaded (one per frame)
    pub pending: Vec<FileMetadata>,
    /// Whether the initial priority load has completed
    pub priority_done: bool,
    /// Generation counter stamped when this queue was built.
    /// Compared against `SpaceLoadGeneration` each frame; mismatch
    /// means a Space switch happened mid-load and the queue is stale.
    pub generation: u64,
}

/// Gate that suppresses `write_instance_changes_system` while the
/// loader is materialising entities from disk. Without it, downstream
/// systems that fire on first-load (mesh-handle resolve →
/// `update_base_part_size_from_mesh`, class-default backfill, material
/// registry resolve) mark `BasePart` as `Changed`, which the writer
/// then persists straight back to disk — at 50k entities that costs
/// ~53 s of background TOML writes for zero useful work.
///
/// Lifecycle:
/// - `load_space_files_system` (Startup) sets `active = true`.
/// - `apply_space_rescan` (Update) sets `active = true` on every rescan.
/// - `open_space` (in `space_ops`) sets `active = true` on Space switch.
/// - `tick_load_in_progress` (Update, runs after `load_deferred_services`)
///   increments `frames_since_quiescent` each frame the deferred queue is
///   empty and `priority_done`. Once the count reaches
///   `QUIESCENT_THRESHOLD`, `active` flips to false and live writes
///   resume.
#[derive(Resource, Debug, Default)]
pub struct LoadInProgress {
    pub active: bool,
    pub frames_since_quiescent: u32,
}

impl LoadInProgress {
    /// Frames the deferred queue must stay empty before declaring the
    /// load truly settled. Sized to absorb the async mesh-handle
    /// resolution + BasePart-size sync that runs for several frames
    /// after the last entity is spawned.
    pub const QUIESCENT_THRESHOLD: u32 = 60;

    /// Mark loading as active. Called by the load entry-points so the
    /// quiescent counter restarts whenever a fresh load begins.
    pub fn begin(&mut self) {
        self.active = true;
        self.frames_since_quiescent = 0;
    }
}

/// Update system: advances the quiescent counter when the deferred
/// queue is empty + priority_done, and flips `active` off once the
/// settle threshold passes. Any frame with pending work resets the
/// counter so a Space switch (or partial rescan) keeps writes gated
/// until the new load also settles.
pub fn tick_load_in_progress(
    deferred: Res<DeferredServiceLoader>,
    mut load: ResMut<LoadInProgress>,
) {
    if !load.active {
        return;
    }
    if deferred.priority_done && deferred.pending.is_empty() {
        load.frames_since_quiescent = load.frames_since_quiescent.saturating_add(1);
        if load.frames_since_quiescent >= LoadInProgress::QUIESCENT_THRESHOLD {
            load.active = false;
            info!(
                "🟢 Load settled — TOML write-back enabled after {} quiescent frames",
                load.frames_since_quiescent
            );
        }
    } else {
        load.frames_since_quiescent = 0;
    }
}

/// Startup system: load only Workspace + Lighting immediately.
/// Other services are queued for deferred loading (one service per frame).
/// Walk `root` recursively and repair the `_instance.toml/_instance.toml`
/// corruption pattern: a directory named `_instance.toml` containing a
/// single file also named `_instance.toml` inside it. This shape arises
/// when a buggy spawner calls `create_dir_all(parent.join("_instance.toml"))`
/// + `fs::write(parent.join("_instance.toml/_instance.toml"), ...)`,
/// which is exactly what happened on 2026-04-25 to `Part-7ed7/`.
///
/// Repair: move the inner file up one level (`<parent>/_instance.toml`)
/// and remove the now-empty wrapper directory. If any unexpected
/// content lives alongside the inner file, leaves it alone and logs a
/// warning rather than risk data loss.
///
/// Returns the number of folders healed. Caller logs the total so a
/// silent repair pass on a clean tree stays quiet.
fn repair_reserved_name_corruption(root: &Path) -> u32 {
    let mut healed = 0u32;
    let mut stack: Vec<std::path::PathBuf> = vec![root.to_path_buf()];

    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else { continue };
            if !file_type.is_dir() { continue; }

            // Recurse first so nested corruption gets healed bottom-up.
            stack.push(path.clone());

            // Is this directory the corrupt-`_instance.toml` shape?
            // (a directory whose name is the reserved marker)
            let dir_name_is_reserved = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.eq_ignore_ascii_case("_instance.toml"))
                .unwrap_or(false);
            if !dir_name_is_reserved { continue; }

            let parent = match path.parent() {
                Some(p) => p,
                None => continue,
            };
            let target = parent.join("_instance.toml");

            // Don't overwrite a real sibling _instance.toml file.
            if target.is_file() {
                tracing::warn!(
                    "🛠 Skipping repair of {:?} — sibling {:?} already exists as a file",
                    path, target
                );
                continue;
            }

            // The wrapper dir should contain exactly one `_instance.toml` file.
            let inner_file = path.join("_instance.toml");
            if !inner_file.is_file() {
                tracing::warn!(
                    "🛠 Skipping repair of {:?} — inner {:?} not present as a file",
                    path, inner_file
                );
                continue;
            }

            // Bail if the wrapper has unexpected siblings (we don't
            // want to silently lose data).
            let extra_count = std::fs::read_dir(&path)
                .map(|d| d.flatten()
                    .filter(|e| e.file_name() != "_instance.toml")
                    .count())
                .unwrap_or(0);
            if extra_count > 0 {
                tracing::warn!(
                    "🛠 Skipping repair of {:?} — has {} unexpected sibling entries",
                    path, extra_count
                );
                continue;
            }

            // Two-step move so we don't ever try to rename a file
            // into the path of its own parent directory (the wrapper).
            let temp = parent.join(".__instance_toml_repair.tmp");
            if let Err(e) = std::fs::rename(&inner_file, &temp) {
                tracing::warn!("🛠 repair: move-out of {:?} failed: {}", inner_file, e);
                continue;
            }
            if let Err(e) = std::fs::remove_dir(&path) {
                tracing::warn!("🛠 repair: rmdir of wrapper {:?} failed: {}", path, e);
                // Try to put the file back so we don't leave a tmp orphan.
                let _ = std::fs::rename(&temp, &inner_file);
                continue;
            }
            if let Err(e) = std::fs::rename(&temp, &target) {
                tracing::warn!("🛠 repair: rename to final {:?} failed: {}", target, e);
                continue;
            }
            healed += 1;
            tracing::info!("🛠 repaired _instance.toml corruption at {:?}", parent);
        }
    }

    healed
}

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
    mut deferred: ResMut<DeferredServiceLoader>,
    gen: Res<SpaceLoadGeneration>,
    mut load_in_progress: ResMut<LoadInProgress>,
    active_source: Res<super::space_source::ActiveSpaceSource>,
) {
    let space_path = &space_root.0;
    let source = active_source.0.clone();
    let source = source.as_ref();

    if !space_path.exists() {
        warn!("Space path does not exist: {:?}", space_path);
        return;
    }

    // Gate TOML write-back until the load settles (see LoadInProgress
    // docstring for the failure mode this prevents).
    load_in_progress.begin();

    // Ensure the Space has all required service folders and lighting TOMLs.
    // This covers the initial-startup path where SpaceRoot is inserted
    // directly (e.g. --space flag or auto-resume) without going through
    // open_space(), which normally calls ensure_space_integrity().
    // Idempotent — never overwrites existing files.
    super::space_ops::ensure_space_integrity(space_path);

    // One-pass repair: walk the Space tree and fix any
    // `<entity>/_instance.toml/_instance.toml` corruption produced by
    // older builds (or external tools) that mistook the marker
    // filename for a folder name. Idempotent — does nothing on a
    // clean tree. Runs before `scan_space_directory` so the loader
    // sees the healed structure.
    //
    // Skipped for a migrated `.eustress` world: there are no loose
    // instance folders on disk to repair (they live in
    // `world.fjalldb/`), so this would only be a wasted full-tree
    // `std::fs` walk on every load.
    if !super::space_ops::space_is_migrated(space_path) {
        let healed = repair_reserved_name_corruption(space_path);
        if healed > 0 {
            warn!(
                "🛠 Repaired {} corrupt _instance.toml folder(s) under {:?}",
                healed, space_path
            );
        }
    }

    let scan_t0 = std::time::Instant::now();
    let entries = scan_space_directory(source, space_path);
    info!(
        target: "eustress_engine::world_db",
        "🔍 Discovered {} top-level entries in Space (scan took {:?})",
        entries.len(),
        scan_t0.elapsed()
    );

    let cd_ref = class_defaults.as_deref();
    let mut deferred_entries = Vec::new();

    // New Space load → discard any spill left from a previous Space and
    // tag the spill generation so a later switch can discard ours.
    {
        let mut q = SPILL.lock().unwrap_or_else(|e| e.into_inner());
        q.clear();
    }
    SPILL_GEN.store(gen.0, std::sync::atomic::Ordering::Relaxed);

    for entry in entries {
        let is_priority = PRIORITY_SERVICES.iter().any(|s| entry.name == *s);

        if is_priority {
            // Load immediately — this is the 3D scene. Frame-budgeted:
            // each priority service gets a fresh budget so a huge one
            // (`Workspace` with the 50k grid) spawns up to
            // SPAWN_BUDGET_PER_FRAME this frame and SPILLS the rest to
            // stream over later frames — no multi-second freeze — while
            // a small one (`Lighting`) still loads fully and instantly.
            // `prio_t0` elapsed should now be small even at 50k.
            let prio_t0 = std::time::Instant::now();
            SPAWN_BUDGET.store(SPAWN_BUDGET_PER_FRAME, std::sync::atomic::Ordering::Relaxed);
            match entry.file_type {
                FileType::Directory => {
                    spawn_directory_entry(
                        &mut commands, &asset_server, &mut meshes, &mut materials,
                        &mut registry, &mut material_registry, &mut mesh_cache, space_path, &entry, None,
                        cd_ref, source,
                    );
                }
                _ => {
                    spawn_file_entry(
                        &mut commands, &asset_server, &mut meshes, &mut materials,
                        &mut registry, &mut material_registry, &mut mesh_cache, space_path, &entry, None,
                        cd_ref, source,
                    );
                }
            }
            let prio_elapsed = prio_t0.elapsed();
            // Always WARN-level so it is visible without RUST_LOG. With
            // the frame budget this should read SMALL even at 50k (only
            // SPAWN_BUDGET_PER_FRAME spawned this frame, rest spilled).
            // A multi-second value here means the budget is NOT being
            // applied on this path.
            warn!(
                target: "eustress_engine::world_db",
                service = %entry.name,
                "⚡ Priority '{}' spawned in {:?} (frame-budgeted — small == fix working; multi-second == budget not applied)",
                entry.name, prio_elapsed
            );
        } else {
            deferred_entries.push(entry);
        }
    }

    info!("📋 Deferred {} services for background loading", deferred_entries.len());
    deferred.pending = deferred_entries;
    deferred.priority_done = true;
    // Stamp the generation so load_deferred_services can detect a
    // mid-load Space switch and discard this queue.
    deferred.generation = gen.0;
}

/// Update system: loads one deferred service per frame to keep the viewport responsive.
pub fn load_deferred_services(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut registry: ResMut<SpaceFileRegistry>,
    mut material_registry: ResMut<super::material_loader::MaterialRegistry>,
    mut mesh_cache: ResMut<super::instance_loader::PrimitiveMeshCache>,
    space_root: Res<super::SpaceRoot>,
    class_defaults: Option<Res<super::class_defaults::ClassDefaultsRegistry>>,
    mut deferred: ResMut<DeferredServiceLoader>,
    gen: Res<SpaceLoadGeneration>,
    active_source: Res<super::space_source::ActiveSpaceSource>,
) {
    if deferred.pending.is_empty() { return; }

    // Generation mismatch — a Space switch (or rename) happened while this
    // queue was draining. Discard the stale entries; open_space already
    // set SpaceRescanNeeded so the new Space will build a fresh queue.
    if deferred.generation != gen.0 {
        let discarded = deferred.pending.len();
        deferred.pending.clear();
        info!("🗑️ Discarded {} stale deferred entries (generation {} → {})",
              discarded, deferred.generation, gen.0);
        return;
    }

    // Load one service per frame
    let entry = deferred.pending.remove(0);
    let space_path = &space_root.0;
    let cd_ref = class_defaults.as_deref();
    let remaining = deferred.pending.len();
    let source = active_source.0.clone();
    let source = source.as_ref();

    match entry.file_type {
        FileType::Directory => {
            spawn_directory_entry(
                &mut commands, &asset_server, &mut meshes, &mut materials,
                &mut registry, &mut material_registry, &mut mesh_cache, space_path, &entry, None,
                cd_ref, source,
            );
        }
        _ => {
            spawn_file_entry(
                &mut commands, &asset_server, &mut meshes, &mut materials,
                &mut registry, &mut material_registry, &mut mesh_cache, space_path, &entry, None,
                cd_ref, source,
            );
        }
    }

    info!("📦 Loaded service: {} ({} remaining)", entry.name, remaining);
}

/// Update system: streams the frame-budget overflow. Each frame it
/// spawns up to [`SPAWN_BUDGET_PER_FRAME`] of the `(node, parent)`
/// pairs that [`spawn_directory_entry`] spilled, so a huge subtree (the
/// 50k benchmark) loads progressively over ~a dozen frames instead of
/// freezing one frame for seconds. Params mirror
/// [`load_deferred_services`] plus `LoadInProgress` (kept armed while
/// streaming so persistence/quiescence don't fire mid-load).
pub fn drain_pending_spawns(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut registry: ResMut<SpaceFileRegistry>,
    mut material_registry: ResMut<super::material_loader::MaterialRegistry>,
    mut mesh_cache: ResMut<super::instance_loader::PrimitiveMeshCache>,
    space_root: Res<super::SpaceRoot>,
    class_defaults: Option<Res<super::class_defaults::ClassDefaultsRegistry>>,
    gen: Res<SpaceLoadGeneration>,
    active_source: Res<super::space_source::ActiveSpaceSource>,
    mut load_in_progress: ResMut<LoadInProgress>,
) {
    use std::sync::atomic::Ordering::Relaxed;

    // Space switch while spill is pending → discard it (it belongs to
    // the previous Space; spawning it now would corrupt the new one).
    if SPILL_GEN.load(Relaxed) != gen.0 {
        let mut q = SPILL.lock().unwrap_or_else(|e| e.into_inner());
        if !q.is_empty() {
            info!("🗑️ Discarded {} stale spilled spawns (Space switch)", q.len());
            q.clear();
        }
        return;
    }

    // Fresh per-frame budget for any recursion the drained nodes do.
    SPAWN_BUDGET.store(SPAWN_BUDGET_PER_FRAME, Relaxed);

    let batch: Vec<(FileMetadata, Option<Entity>)> = {
        let mut q = SPILL.lock().unwrap_or_else(|e| e.into_inner());
        if q.is_empty() {
            return;
        }
        let n = q.len().min(SPAWN_BUDGET_PER_FRAME as usize);
        q.drain(..n).collect()
    };

    // Still loading — keep persistence + the LoadInProgress quiescence
    // window armed until the spill is fully drained.
    load_in_progress.begin();

    let space_path = &space_root.0;
    let cd_ref = class_defaults.as_deref();
    let source = active_source.0.clone();
    let source = source.as_ref();
    let count = batch.len();

    for (meta, parent) in batch {
        match meta.file_type {
            FileType::Directory => {
                spawn_directory_entry(
                    &mut commands, &asset_server, &mut meshes, &mut materials,
                    &mut registry, &mut material_registry, &mut mesh_cache, space_path, &meta, parent,
                    cd_ref, source,
                );
            }
            _ => {
                spawn_file_entry(
                    &mut commands, &asset_server, &mut meshes, &mut materials,
                    &mut registry, &mut material_registry, &mut mesh_cache, space_path, &meta, parent,
                    cd_ref, source,
                );
            }
        }
    }

    let remaining = SPILL.lock().unwrap_or_else(|e| e.into_inner()).len();
    warn!(
        target: "eustress_engine::world_db",
        unique_materials = material_registry.dedup_cache_len(),
        dense_quant = super::material_loader::dense_material_mode(),
        "🧩 Streamed {} spilled spawns ({} remaining). unique_materials is the draw-call batch count — if it's ~50k the render ceiling stands; with dense_quant=true it should be only a few thousand (≈12× fewer draws).",
        count, remaining
    );
}

/// Plugin for dynamic file loading
pub struct SpaceFileLoaderPlugin;

impl Plugin for SpaceFileLoaderPlugin {
    fn build(&self, app: &mut App) {
        // Note: The "space://" asset source is registered in main.rs BEFORE DefaultPlugins
        // This must happen before AssetPlugin is initialized, so we can't do it here.
        
        app.init_resource::<super::SpaceRoot>()
            .init_resource::<SpaceFileRegistry>()
            .init_resource::<SpaceLoadGeneration>()
            .init_resource::<super::material_loader::MaterialRegistry>()
            .init_resource::<super::instance_loader::PrimitiveMeshCache>()
            .init_resource::<super::file_watcher::RecentlyWrittenFiles>()
            .init_resource::<super::space_ops::SpaceRescanNeeded>()
            .init_resource::<DeferredServiceLoader>()
            .init_resource::<LoadInProgress>()
            // Content source for the loader — Disk by default; the
            // world-db plugin swaps it to Fjall on Space open once the
            // tree is seeded. Registered here (not behind the feature)
            // so loader systems can always read through it.
            .init_resource::<super::space_source::ActiveSpaceSource>()
            // Class schema — common-crate source of truth for every
            // `_instance.toml`. Embedded templates normalise to PascalCase,
            // `load_and_heal_instance` fills missing fields + self-heals
            // the file on disk. Plugins extend the schema by registering
            // `ExtraSectionClaim` impls on `ExtraSectionRegistry`.
            .init_resource::<eustress_common::class_schema::ClassSchemaResource>()
            .init_resource::<eustress_common::class_schema::ExtraSectionRegistry>()
            // Single canonical filesystem-change broadcast. Sourced by
            // `SpaceFileWatcher`'s notify thread, consumed by any
            // subsystem that needs to react to disk changes (streaming
            // spatial grid, plugins, future tooling). See
            // `common::file_events` for the design rationale.
            .add_message::<eustress_common::file_events::FileChanged>()
            .add_systems(Startup, (
                // Register every first-party ExtraSectionClaim that
                // ships with common (currently just ThermodynamicClaim).
                // Plugins that want to add more claimants do the same
                // dance in their own `build` after this runs.
                |mut registry: ResMut<eustress_common::class_schema::ExtraSectionRegistry>| {
                    registry.register_builtins();
                },
                super::class_defaults::startup_load_class_defaults,
                // Template / enum / filename-stem drift check. Runs
                // once at boot, logs warnings for any class template
                // whose `[metadata] class_name` doesn't match its
                // file stem, or whose stem isn't in the ClassName
                // enum. Catches "added a template but forgot the
                // enum variant" bugs at startup instead of at
                // load-a-Part time.
                eustress_common::class_schema::log_schema_validation,
                load_space_files_system.after(crate::default_scene::setup_default_scene),
                super::file_watcher::setup_file_watcher,
            ))
            .add_systems(Update, (
                load_deferred_services,
                drain_pending_spawns.after(load_deferred_services),
                tick_load_in_progress.after(drain_pending_spawns),
                super::file_watcher::process_file_changes,
                super::instance_loader::ensure_tags_and_attributes_components,
                super::instance_loader::ensure_measure_unit,
                // Live: edited Workspace `render_distance` service
                // property → part VisibilityRange. Changed-gated.
                super::instance_loader::sync_workspace_render_distance,
                super::space_ops::apply_space_rescan,
                super::instance_loader::update_base_part_size_from_mesh,
                // Per-frame safety net: clamp NaN/Inf and sky-distance
                // overflow on every Avian-tracked Transform so any
                // drag-tool / plugin that writes a degenerate value
                // can't reach Avian's `assert_components_finite` and
                // panic the engine. Tools should clamp at the source
                // via `safe_translation`, but this catches misses.
                super::instance_loader::sanitize_part_transforms_safety_net,
                // Hand each freshly-loaded instance's extra
                // `[section]`s to any plugin that registered for them
                // via `ExtraSectionRegistry`. Runs once per entity on
                // the frame `PendingExtraSections` is inserted, then
                // removes the component.
                eustress_common::class_schema::dispatch_pending_extras,
            ));

        // Legacy TOML write-back — only when the `toml` feature is on.
        // Default build (ECS+DB authoritative, 2026-05-15 pivot) does
        // NOT register these: runtime edits persist to `world.fjalldb/`
        // via `world_db_plugin`, not to `_instance.toml`. The TOML
        // *read* path above stays compiled regardless — it's the
        // first-run Fjall seed + recovery loader.
        #[cfg(feature = "toml")]
        {
            app.add_systems(
                Update,
                (
                    super::instance_loader::write_instance_changes_system,
                    super::instance_loader::save_tags_and_attributes_changes,
                ),
            );
        }
    }
}

/// Unit quad mesh used by Image / Video classes. UVs are origin-top-left
/// to match the convention the rest of the engine uses (BillboardCard,
/// Decal). Scaled by `Transform::from_scale(Vec3::new(width, height, 1))`
/// so the same mesh handle is shared across every imported media entity.
fn build_imported_media_quad() -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-0.5, -0.5, 0.0],
            [ 0.5, -0.5, 0.0],
            [ 0.5,  0.5, 0.0],
            [-0.5,  0.5, 0.0],
        ],
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4]);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![
            [0.0, 1.0],   // bottom-left
            [1.0, 1.0],   // bottom-right
            [1.0, 0.0],   // top-right
            [0.0, 0.0],   // top-left
        ],
    );
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}
