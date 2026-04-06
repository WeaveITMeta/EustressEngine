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
    /// Timestamp when the watcher was created — used to ignore spurious
    /// Modify events that `notify` fires for pre-existing files on startup.
    created_at: std::time::Instant,
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
            created_at: std::time::Instant::now(),
        })
    }
    
    /// Poll for file events (non-blocking)
    pub fn poll_events(&self) -> Vec<FileChangeEvent> {
        let _start = std::time::Instant::now();
        let mut events = Vec::new();
        let mut raw_event_count = 0;
        
        // Drain all pending events
        while let Ok(result) = self.receiver.try_recv() {
            match result {
                Ok(debounced_events) => {
                    raw_event_count += debounced_events.len();
                    for event in debounced_events {
                        if let Some(change_event) = self.process_event(event.event) {
                            events.push(change_event);
                        }
                    }
                }
                Err(errors) => {
                    for err in errors {
                        error!("File watcher error: {}", err);
                    }
                }
            }
        }
        
        let elapsed = _start.elapsed();
        if raw_event_count > 0 {
            warn!("🔍 File watcher received {} raw events, processed {} change events in {:.1}ms", 
                raw_event_count, events.len(), elapsed.as_secs_f64() * 1000.0);
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
        
        // For Remove events, file type is irrelevant — we just need the path
        // to look up the entity in the registry and despawn it.
        // For other events, determine file type from path/extension.
        let file_type = if change_type == FileChangeType::Removed {
            FileType::Toml // placeholder — not used for removal, just needs a value
        } else {
            FileType::from_path(&path)
                .or_else(|| path.extension().and_then(|e| e.to_str()).and_then(FileType::from_extension))?
        };
        
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
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut material_registry: ResMut<super::material_loader::MaterialRegistry>,
    mut mesh_cache: ResMut<super::instance_loader::PrimitiveMeshCache>,
    mut recently_written: ResMut<RecentlyWrittenFiles>,
    space_root: Res<super::SpaceRoot>,
    // Query for entities loaded from files
    file_entities: Query<(Entity, &super::file_loader::LoadedFromFile)>,
    // Query for Soul scripts
    mut soul_scripts: Query<&mut crate::soul::SoulScriptData>,
    class_defaults: Option<Res<super::class_defaults::ClassDefaultsRegistry>>,
    mut asset_manager_state: Option<ResMut<crate::ui::slint_ui::AssetManagerState>>,
    mut explorer_state: Option<ResMut<crate::ui::slint_ui::UnifiedExplorerState>>,
) {
    let _start = std::time::Instant::now();
    let Some(watcher) = watcher else {
        return;
    };
    
    // Clean up old entries from recently written files
    recently_written.cleanup();

    // Periodic stale entity cleanup: every ~300 frames (~5s at 60fps), check if
    // any file-loaded entities reference files that no longer exist on disk.
    // This catches deletions the watcher might have missed (e.g. bulk delete,
    // deletion before watcher was initialized, or directory removal).
    static CLEANUP_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let counter = CLEANUP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if counter % 300 == 0 {
        let mut stale: Vec<(Entity, std::path::PathBuf)> = Vec::new();
        for (entity, loaded) in file_entities.iter() {
            if !loaded.path.exists() {
                stale.push((entity, loaded.path.clone()));
            }
        }
        for (entity, path) in stale {
            info!("🧹 Stale entity cleanup: despawning {:?} (file deleted: {:?})", entity, path);
            commands.entity(entity).despawn();
            registry.unregister_file(&path);
        }
    }
    
    let events = watcher.poll_events();
    
    if !events.is_empty() {
        // Mark asset manager and explorer caches stale so they rescan on next sync
        if let Some(ref mut ams) = asset_manager_state {
            ams.cache_stale = true;
            ams.dirty = true;
        }
        if let Some(ref mut es) = explorer_state {
            es.explorer_fs_stale = true;
            es.needs_immediate_sync = true;
        }

        let elapsed = _start.elapsed();
        if elapsed.as_millis() > 50 {
            warn!("🐌 process_file_changes took {:.1}ms ({} events)", elapsed.as_secs_f64() * 1000.0, events.len());
        }
    }
    
    // Grace period: ignore Modified events for the first 5 seconds after watcher
    // creation. `notify` fires spurious Modify events for pre-existing files when
    // the watcher starts — those files were already loaded by load_space_files_system.
    let in_grace_period = watcher.created_at.elapsed() < Duration::from_secs(5);

    for event in events {
        // Skip files that were recently written by the engine (prevents hot-reload loops)
        if recently_written.was_recently_written(&event.path) {
            debug!("Skipping hot-reload for recently written file: {:?}", event.path);
            continue;
        }
        
        match event.change_type {
            FileChangeType::Modified => {
                // During startup grace period, skip spurious modify events
                if in_grace_period {
                    continue;
                }
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
                handle_file_created(&event, &mut registry, &mut material_registry, &mut mesh_cache, &mut commands, &asset_server, &mut materials, &space_root.0, class_defaults.as_deref());
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
        FileType::Soul | FileType::Rune => {
            // Hot-reload Soul/Rune script source
            if let Some(entity) = registry.get_entity(&event.path) {
                if let Ok(mut script_data) = soul_scripts.get_mut(entity) {
                    match std::fs::read_to_string(&event.path) {
                        Ok(new_source) => {
                            script_data.source = new_source;
                            script_data.dirty = true;
                            script_data.build_status = crate::soul::SoulBuildStatus::Stale;

                            info!("🔄 Hot-reloaded script: {:?}", event.path);

                            commands.trigger(crate::soul::TriggerBuildEvent {
                                entity,
                            });
                        }
                        Err(e) => {
                            error!("Failed to reload script {:?}: {}", event.path, e);
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
                                    
                                    debug!("🔄 Hot-reloaded TOML instance: {:?}", event.path);
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
        
        FileType::GuiElement => {
            // Hot-reload GUI element TOML (Frame, TextLabel, TextButton, etc.)
            if let Some(entity) = registry.get_entity(&event.path) {
                match super::gui_loader::load_gui_definition(&event.path) {
                    Ok(gui_def) => {
                        let gui_type = super::gui_loader::gui_class_from_extension(&event.path);
                        let display = super::gui_loader::gui_display_from_props(
                            &gui_def.gui,
                            gui_def.text.as_ref(),
                            gui_type,
                        );
                        commands.entity(entity).insert(display);
                        info!("🔄 Hot-reloaded GUI element: {:?}", event.path);
                    }
                    Err(e) => {
                        error!("Failed to reload GUI element {:?}: {}", event.path, e);
                    }
                }
            }
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
    material_registry: &mut super::material_loader::MaterialRegistry,
    mesh_cache: &mut super::instance_loader::PrimitiveMeshCache,
    commands: &mut Commands,
    asset_server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
    space_root: &std::path::Path,
    class_defaults: Option<&super::class_defaults::ClassDefaultsRegistry>,
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
        
        FileType::Soul | FileType::Rune => {
            match std::fs::read_to_string(&event.path) {
                Ok(source) => {
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
                            source,
                            dirty: false,
                            ast: None,
                            generated_code: None,
                            build_status: crate::soul::SoulBuildStatus::NotBuilt,
                            errors: Vec::new(),
                            run_context: Default::default(),
                        },
                        super::file_loader::LoadedFromFile {
                            path: event.path.clone(),
                            file_type: event.file_type,
                            service: event.service.clone(),
                        },
                        Name::new(name.clone()),
                    )).id();

                    // Parent to service entity so the Explorer primary path finds it
                    let service_toml = space_root.join(&event.service).join("_service.toml");
                    if let Some(service_entity) = registry.get_entity(&service_toml) {
                        commands.entity(entity).insert(ChildOf(service_entity));
                    }

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
                    info!("➕ Loaded new {} script: {:?}",
                        if event.file_type == FileType::Rune { "Rune" } else { "Soul" },
                        event.path);
                }
                Err(e) => {
                    error!("Failed to read new script {:?}: {}", event.path, e);
                }
            }
        }

        FileType::Toml => {
            // Load .part.toml, .model.toml, .instance.toml files
            match super::instance_loader::load_instance_definition_with_defaults(&event.path, class_defaults) {
                Ok(instance) => {
                    let entity = super::instance_loader::spawn_instance(
                        commands,
                        asset_server,
                        materials,
                        material_registry,
                        mesh_cache,
                        event.path.clone(),
                        instance,
                    );
                    
                    let name = event.path.file_stem()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Unknown")
                        .to_string();
                    
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
                    
                    info!("✅ Loaded new instance file: {:?}", event.path);
                }
                Err(e) => {
                    error!("Failed to load new instance file {:?}: {}", event.path, e);
                }
            }
        }
        
        FileType::GuiElement => {
            // Hot-load new GUI element TOML (TextLabel, TextButton, Frame, etc.)
            match super::gui_loader::load_gui_definition(&event.path) {
                Ok(gui_def) => {
                    let gui_type = super::gui_loader::gui_class_from_extension(&event.path);
                    let display = super::gui_loader::gui_display_from_props(
                        &gui_def.gui,
                        gui_def.text.as_ref(),
                        gui_type,
                    );
                    let name = if !gui_def.instance.name.is_empty() {
                        gui_def.instance.name.clone()
                    } else {
                        event.path.file_stem()
                            .and_then(|n| n.to_str())
                            .unwrap_or("Unknown")
                            .to_string()
                    };

                    let class_name = super::gui_loader::gui_class_name_from_type(gui_type);

                    let entity = commands.spawn((
                        eustress_common::classes::Instance {
                            name: name.clone(),
                            class_name,
                            archivable: true,
                            id: 0,
                            ai: false,
                        },
                        display,
                        Node { display: Display::None, ..default() },
                        super::file_loader::LoadedFromFile {
                            path: event.path.clone(),
                            file_type: event.file_type,
                            service: event.service.clone(),
                        },
                        Name::new(name.clone()),
                    )).id();

                    // Parent to containing directory entity if it exists
                    if let Some(parent_dir) = event.path.parent() {
                        let parent_instance = parent_dir.join("_instance.toml");
                        if let Some(parent_entity) = registry.get_entity(&parent_instance) {
                            commands.entity(entity).insert(ChildOf(parent_entity));
                        } else {
                            // Try parent service
                            let service_toml = space_root.join(&event.service).join("_service.toml");
                            if let Some(service_entity) = registry.get_entity(&service_toml) {
                                commands.entity(entity).insert(ChildOf(service_entity));
                            }
                        }
                    }

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
                    info!("➕ Loaded new GUI element: {:?}", event.path);
                }
                Err(e) => {
                    error!("Failed to load new GUI element {:?}: {}", event.path, e);
                }
            }
        }

        FileType::Material => {
            // Hot-load new .mat.toml files into MaterialRegistry
            match super::material_loader::load_material_definition(&event.path) {
                Ok(definition) => {
                    let mat_name = if definition.material.name.is_empty() {
                        super::material_loader::material_name_from_path(&event.path)
                    } else {
                        definition.material.name.clone()
                    };
                    let mat_toml_dir = event.path.parent().unwrap_or(std::path::Path::new("."));
                    let standard_mat = super::material_loader::build_standard_material(
                        &definition,
                        asset_server,
                        mat_toml_dir,
                        space_root,
                    );
                    let handle = materials.add(standard_mat);
                    material_registry.insert(
                        mat_name.clone(),
                        handle,
                        definition.clone(),
                        event.path.clone(),
                    );
                    let entity = super::material_loader::spawn_material_entity(
                        commands,
                        event.path.clone(),
                        &definition,
                    );
                    registry.register(
                        event.path.clone(),
                        entity,
                        super::file_loader::FileMetadata {
                            path: event.path.clone(),
                            file_type: event.file_type,
                            service: event.service.clone(),
                            name: mat_name.clone(),
                            size: 0,
                            modified: std::time::SystemTime::now(),
                            children: Vec::new(),
                        },
                    );
                    info!("🎨 Hot-loaded new material '{}' from {:?}", mat_name, event.path);
                }
                Err(e) => {
                    error!("Failed to load new material {:?}: {}", event.path, e);
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
