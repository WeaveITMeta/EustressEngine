//! File watcher for hot-reload of Space files
//!
//! Watches for changes to .soul, .glb, and other files in the Space directory
//! and automatically reloads them when modified externally.

use bevy::prelude::*;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, FileIdMap};
use std::path::{Path, PathBuf};
use crossbeam_channel::{unbounded, Receiver};
use std::time::Duration;

use super::file_loader::{FileType, SpaceFileRegistry};

/// File watcher resource
#[derive(Resource)]
pub struct SpaceFileWatcher {
    /// Debounced watcher
    _watcher: Debouncer<RecommendedWatcher, FileIdMap>,
    /// Channel receiver for file events
    receiver: Receiver<DebounceEventResult>,
    /// Space root path being watched
    space_path: PathBuf,
}

impl SpaceFileWatcher {
    /// Create a new file watcher for the given Space path
    pub fn new(space_path: PathBuf) -> Result<Self, String> {
        let (tx, rx) = unbounded();
        
        // Create debounced watcher (300ms debounce to avoid rapid fire events)
        let mut debouncer = new_debouncer(
            Duration::from_millis(300),
            None,
            move |result: DebounceEventResult| {
                if let Err(e) = tx.send(result) {
                    error!("Failed to send file event: {}", e);
                }
            },
        ).map_err(|e| format!("Failed to create file watcher: {}", e))?;
        
        // Watch the Space directory recursively
        debouncer
            .watcher()
            .watch(&space_path, RecursiveMode::Recursive)
            .map_err(|e| format!("Failed to watch directory: {}", e))?;
        
        info!("👁 File watcher started for: {:?}", space_path);
        
        Ok(Self {
            _watcher: debouncer,
            receiver: rx,
            space_path,
        })
    }
    
    /// Poll for file events (non-blocking)
    pub fn poll_events(&self) -> Vec<FileChangeEvent> {
        let mut events = Vec::new();
        
        // Drain all pending events
        while let Ok(result) = self.receiver.try_recv() {
            match result {
                Ok(debounced_events) => {
                    for event in debounced_events {
                        if let Some(change_event) = self.process_event(event.event) {
                            events.push(change_event);
                        }
                    }
                }
                Err(errors) => {
                    for error in errors {
                        warn!("File watcher error: {}", error);
                    }
                }
            }
        }
        
        events
    }
    
    /// Process a raw notify event into a FileChangeEvent
    fn process_event(&self, event: Event) -> Option<FileChangeEvent> {
        // Only care about modify and create events
        let change_type = match event.kind {
            EventKind::Modify(_) => FileChangeType::Modified,
            EventKind::Create(_) => FileChangeType::Created,
            EventKind::Remove(_) => FileChangeType::Removed,
            _ => return None,
        };
        
        // Get the first path (notify can have multiple paths per event)
        let path = event.paths.first()?.clone();
        
        // Skip if not a file
        if !path.is_file() && change_type != FileChangeType::Removed {
            return None;
        }
        
        // Determine file type
        let ext = path.extension()?.to_str()?;
        let file_type = FileType::from_extension(ext)?;
        
        // Determine service from path
        let service = self.extract_service_from_path(&path)?;
        
        Some(FileChangeEvent {
            path,
            file_type,
            service,
            change_type,
        })
    }
    
    /// Extract service name from file path
    fn extract_service_from_path(&self, path: &Path) -> Option<String> {
        // Get relative path from space root
        let relative = path.strip_prefix(&self.space_path).ok()?;
        
        // First component should be the service name
        let service = relative.components().next()?.as_os_str().to_str()?;
        
        Some(service.to_string())
    }
}

/// File change event
#[derive(Debug, Clone)]
pub struct FileChangeEvent {
    pub path: PathBuf,
    pub file_type: FileType,
    pub service: String,
    pub change_type: FileChangeType,
}

/// Type of file change
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeType {
    Created,
    Modified,
    Removed,
}

/// Resource to track files recently written by the engine (to avoid hot-reload loops)
#[derive(Resource, Default)]
pub struct RecentlyWrittenFiles {
    /// Map of file path to the time it was written
    pub files: std::collections::HashMap<PathBuf, std::time::Instant>,
}

impl RecentlyWrittenFiles {
    /// Mark a file as recently written
    pub fn mark_written(&mut self, path: PathBuf) {
        self.files.insert(path, std::time::Instant::now());
    }
    
    /// Check if a file was recently written (within the last 2 seconds)
    /// Extended window to prevent hot-reload loops when Transform changes trigger writes
    pub fn was_recently_written(&self, path: &Path) -> bool {
        if let Some(time) = self.files.get(path) {
            time.elapsed() < std::time::Duration::from_millis(2000)
        } else {
            false
        }
    }
    
    /// Clean up old entries (older than 2 seconds)
    pub fn cleanup(&mut self) {
        let cutoff = std::time::Duration::from_secs(2);
        self.files.retain(|_, time| time.elapsed() < cutoff);
    }
}

/// System to process file change events and hot-reload
pub fn process_file_changes(
    watcher: Option<Res<SpaceFileWatcher>>,
    mut registry: ResMut<SpaceFileRegistry>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut recently_written: ResMut<RecentlyWrittenFiles>,
    // Query for entities loaded from files
    file_entities: Query<(Entity, &super::file_loader::LoadedFromFile)>,
    // Query for Soul scripts
    mut soul_scripts: Query<&mut crate::soul::SoulScriptData>,
) {
    let Some(watcher) = watcher else {
        return;
    };
    
    // Clean up old entries from recently written files
    recently_written.cleanup();
    
    let events = watcher.poll_events();
    
    for event in events {
        // Skip files that were recently written by the engine (prevents hot-reload loops)
        if recently_written.was_recently_written(&event.path) {
            debug!("Skipping hot-reload for recently written file: {:?}", event.path);
            continue;
        }
        
        match event.change_type {
            FileChangeType::Modified => {
                // Mark as recently written BEFORE hot-reload to prevent write-back loop
                // When we hot-reload and insert Transform, it triggers Changed<Transform>,
                // which would trigger write_instance_changes_system. By marking it here,
                // that system will skip writing this file.
                recently_written.mark_written(event.path.clone());
                
                handle_file_modified(
                    &event,
                    &mut registry,
                    &mut commands,
                    &asset_server,
                    &file_entities,
                    &mut soul_scripts,
                );
            }
            FileChangeType::Created => {
                handle_file_created(&event, &mut registry, &mut commands, &asset_server);
            }
            FileChangeType::Removed => {
                handle_file_removed(&event, &mut registry, &mut commands);
            }
        }
    }
}

/// Handle file modification (hot-reload)
fn handle_file_modified(
    event: &FileChangeEvent,
    registry: &mut SpaceFileRegistry,
    commands: &mut Commands,
    asset_server: &AssetServer,
    file_entities: &Query<(Entity, &super::file_loader::LoadedFromFile)>,
    soul_scripts: &mut Query<&mut crate::soul::SoulScriptData>,
) {
    match event.file_type {
        FileType::Soul => {
            // Hot-reload Soul script
            if let Some(entity) = registry.get_entity(&event.path) {
                if let Ok(mut script_data) = soul_scripts.get_mut(entity) {
                    // Reload markdown source
                    match std::fs::read_to_string(&event.path) {
                        Ok(new_source) => {
                            script_data.source = new_source;
                            script_data.dirty = true;
                            script_data.build_status = crate::soul::SoulBuildStatus::Stale;
                            
                            info!("🔄 Hot-reloaded Soul script: {:?}", event.path);
                            
                            // Trigger rebuild
                            commands.trigger(crate::soul::TriggerBuildEvent {
                                entity,
                            });
                        }
                        Err(e) => {
                            error!("Failed to reload Soul script {:?}: {}", event.path, e);
                        }
                    }
                }
            }
        }
        
        FileType::Gltf => {
            // Hot-reload glTF/GLB model
            if let Some(entity) = registry.get_entity(&event.path) {
                // Find the entity with this file
                for (ent, loaded) in file_entities.iter() {
                    if ent == entity && loaded.path == event.path {
                        // Reload the scene
                        let scene_handle = asset_server.load(format!("{}#Scene0", event.path.display()));
                        commands.entity(entity).insert(SceneRoot(scene_handle));
                        
                        info!("🔄 Hot-reloaded glTF model: {:?}", event.path);
                        break;
                    }
                }
            }
        }
        
        FileType::Toml => {
            // Hot-reload TOML instance file (.glb.toml, .part.toml, etc.)
            let path_str = event.path.to_string_lossy();
            if path_str.ends_with(".glb.toml") 
                || path_str.ends_with(".part.toml") 
                || path_str.ends_with(".model.toml")
                || path_str.ends_with(".instance.toml") 
            {
                if let Some(entity) = registry.get_entity(&event.path) {
                    // Reload the TOML and update ECS components
                    match std::fs::read_to_string(&event.path) {
                        Ok(toml_content) => {
                            match toml::from_str::<crate::space::instance_loader::InstanceDefinition>(&toml_content) {
                                Ok(instance_def) => {
                                    // Update transform
                                    let transform: Transform = instance_def.transform.into();
                                    commands.entity(entity).insert(transform);
                                    
                                    // Update realism components if present (use to_component() methods)
                                    if let Some(ref mat) = instance_def.material {
                                        commands.entity(entity).insert(mat.to_component());
                                    }
                                    
                                    if let Some(ref thermo) = instance_def.thermodynamic {
                                        commands.entity(entity).insert(thermo.to_component());
                                    }
                                    
                                    if let Some(ref echem) = instance_def.electrochemical {
                                        commands.entity(entity).insert(echem.to_component());
                                    }
                                    
                                    info!("🔄 Hot-reloaded TOML instance: {:?}", event.path);
                                }
                                Err(e) => {
                                    error!("Failed to parse TOML instance {:?}: {}", event.path, e);
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to read TOML instance {:?}: {}", event.path, e);
                        }
                    }
                }
            }
        }
        
        FileType::Png | FileType::Jpg | FileType::Tga => {
            // Hot-reload texture
            // Bevy's asset server handles this automatically via hot-reload
            info!("🔄 Texture changed (Bevy will auto-reload): {:?}", event.path);
        }
        
        _ => {
            debug!("File modified but no hot-reload handler: {:?}", event.path);
        }
    }
}

/// Handle new file creation
fn handle_file_created(
    event: &FileChangeEvent,
    registry: &mut SpaceFileRegistry,
    commands: &mut Commands,
    asset_server: &AssetServer,
) {
    // Check if file type should spawn an entity
    if !event.file_type.spawns_entity_in_service(&event.service) {
        return;
    }
    
    // Check if already loaded
    if registry.is_loaded(&event.path) {
        return;
    }
    
    info!("➕ New file detected: {:?}", event.path);
    
    // Load the new file (same logic as initial scan)
    match event.file_type {
        FileType::Gltf => {
            let scene_handle = asset_server.load(format!("{}#Scene0", event.path.display()));
            let name = event.path.file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string();
            
            let entity = commands.spawn((
                SceneRoot(scene_handle),
                Transform::default(),
                eustress_common::classes::Instance {
                    name: name.clone(),
                    class_name: eustress_common::classes::ClassName::Part,
                    archivable: true,
                    id: 0,
                    ai: false,
                },
                eustress_common::default_scene::PartEntityMarker {
                    part_id: name.clone(),
                },
                super::file_loader::LoadedFromFile {
                    path: event.path.clone(),
                    file_type: event.file_type,
                    service: event.service.clone(),
                },
                Name::new(name.clone()),
            )).id();
            
            registry.register(
                event.path.clone(),
                entity,
                super::file_loader::FileMetadata {
                    path: event.path.clone(),
                    file_type: event.file_type,
                    service: event.service.clone(),
                    name,
                    size: 0,
                    modified: std::time::SystemTime::now(),
                    children: Vec::new(),
                },
            );
        }
        
        FileType::Soul => {
            match std::fs::read_to_string(&event.path) {
                Ok(markdown_source) => {
                    let name = event.path.file_stem()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Unknown")
                        .to_string();
                    
                    let entity = commands.spawn((
                        eustress_common::classes::Instance {
                            name: name.clone(),
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
                        super::file_loader::LoadedFromFile {
                            path: event.path.clone(),
                            file_type: event.file_type,
                            service: event.service.clone(),
                        },
                        Name::new(name.clone()),
                    )).id();
                    
                    registry.register(
                        event.path.clone(),
                        entity,
                        super::file_loader::FileMetadata {
                            path: event.path.clone(),
                            file_type: event.file_type,
                            service: event.service.clone(),
                            name,
                            size: 0,
                            modified: std::time::SystemTime::now(),
                            children: Vec::new(),
                        },
                    );
                }
                Err(e) => {
                    error!("Failed to read new Soul script {:?}: {}", event.path, e);
                }
            }
        }
        
        _ => {}
    }
}

/// Handle file deletion
fn handle_file_removed(
    event: &FileChangeEvent,
    registry: &mut SpaceFileRegistry,
    commands: &mut Commands,
) {
    if let Some(entity) = registry.get_entity(&event.path) {
        info!("➖ File deleted, despawning entity: {:?}", event.path);
        commands.entity(entity).despawn();
        registry.unregister_file(&event.path);
    }
}

/// Initialize file watcher on startup
pub fn setup_file_watcher(
    mut commands: Commands,
    space_root: Res<super::SpaceRoot>,
) {
    let space_path = space_root.0.clone();
    
    if !space_path.exists() {
        warn!("Space path does not exist, file watcher disabled: {:?}", space_path);
        return;
    }
    
    match SpaceFileWatcher::new(space_path) {
        Ok(watcher) => {
            commands.insert_resource(watcher);
            info!("✅ File watcher initialized");
        }
        Err(e) => {
            error!("❌ Failed to initialize file watcher: {}", e);
        }
    }
}
