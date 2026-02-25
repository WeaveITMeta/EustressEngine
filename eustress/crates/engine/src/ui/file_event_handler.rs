//! # File Event Handler
//!
//! Consumes `FileEvent` messages dispatched by the Slint UI (Save, Open, New, etc.)
//! and executes the appropriate serialization operations.
//!
//! ## Table of Contents
//!
//! 1. **PendingFileActions** — Resource bridging MessageReader → exclusive system
//! 2. **drain_file_events** — Regular system that reads FileEvent messages into resource
//! 3. **execute_file_actions** — Exclusive system that performs save/load with &mut World
//! 4. **do_save_scene** — Save current scene to binary .eustress
//! 5. **do_save_scene_as** — Save with file picker dialog
//! 6. **do_open_scene** — Open binary .eustress via file picker
//! 7. **do_new_scene** — Clear world and create fresh default scene

use bevy::prelude::*;
use bevy::ecs::message::MessageReader;
use std::path::PathBuf;

use super::file_dialogs::{FileEvent, SceneFile, pick_open_file, pick_save_file};
use crate::notifications::NotificationManager;

// ============================================================================
// 1. Pending File Actions Resource (bridges regular → exclusive systems)
// ============================================================================

/// Staging resource: regular system drains FileEvent messages here,
/// then the exclusive system picks them up with full World access.
#[derive(Resource, Default)]
pub struct PendingFileActions {
    pub actions: Vec<FileAction>,
}

/// Owned version of FileEvent (FileEvent has PathBuf which needs Clone)
#[derive(Clone, Debug)]
pub enum FileAction {
    SaveScene,
    SaveSceneAs,
    OpenScene,
    OpenRecent(PathBuf),
    NewScene,
    Publish,
}

// ============================================================================
// 2. Regular system: drain FileEvent messages into PendingFileActions
// ============================================================================

/// Reads FileEvent messages and stages them for the exclusive system.
pub fn drain_file_events(
    mut events: MessageReader<FileEvent>,
    mut pending: ResMut<PendingFileActions>,
) {
    for event in events.read() {
        let action = match event {
            FileEvent::SaveScene => FileAction::SaveScene,
            FileEvent::SaveSceneAs => FileAction::SaveSceneAs,
            FileEvent::OpenScene => FileAction::OpenScene,
            FileEvent::OpenRecent(path) => FileAction::OpenRecent(path.clone()),
            FileEvent::NewScene => FileAction::NewScene,
            FileEvent::Publish | FileEvent::PublishAs => FileAction::Publish,
        };
        pending.actions.push(action);
    }
}

// ============================================================================
// 3. Exclusive system: execute pending file actions with full World access
// ============================================================================

/// Processes staged file actions. Requires `&mut World` because binary
/// save/load need exclusive access (same pattern as play_mode.rs).
pub fn execute_file_actions(world: &mut World) {
    // Take actions out of the resource (avoids borrow issues)
    let actions: Vec<FileAction> = {
        let Some(mut pending) = world.get_resource_mut::<PendingFileActions>() else {
            return;
        };
        std::mem::take(&mut pending.actions)
    };

    if actions.is_empty() {
        return;
    }

    for action in actions {
        match action {
            FileAction::SaveScene => do_save_scene(world),
            FileAction::SaveSceneAs => do_save_scene_as(world),
            FileAction::OpenScene => do_open_scene(world),
            FileAction::OpenRecent(path) => do_open_scene_path(world, &path),
            FileAction::NewScene => do_new_scene(world),
            FileAction::Publish => {
                if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
                    notifs.warning("Publish is not yet implemented — use Save Scene instead");
                }
            }
        }
    }
}

// ============================================================================
// 2. Save Scene — save to current path, or prompt if untitled
// ============================================================================

/// Save the current scene to its existing path (binary format).
/// Falls through to SaveAs if no path is set.
fn do_save_scene(world: &mut World) {
    // Check if we have a current scene path
    let save_path = {
        let scene_file = world.get_resource::<SceneFile>();
        scene_file.and_then(|sf| sf.path.clone())
    };

    match save_path {
        Some(path) => {
            save_to_path(world, &path);
        }
        None => {
            // No path yet — fall through to SaveAs
            do_save_scene_as(world);
        }
    }
}

// ============================================================================
// 3. Save Scene As — prompt file picker, then save
// ============================================================================

/// Prompt the user for a file path, then save.
fn do_save_scene_as(world: &mut World) {
    // Show native file picker (blocking, but it's a system dialog)
    let path = pick_save_file();

    if let Some(path) = path {
        save_to_path(world, &path);
    } else {
        info!("💾 Save cancelled by user");
    }
}

/// Perform the actual binary save to a given path.
fn save_to_path(world: &mut World, path: &PathBuf) {
    info!("💾 Saving scene to {:?}", path);

    match crate::serialization::save_binary_scene(world, path) {
        Ok(()) => {
            // Update SceneFile resource
            if let Some(mut scene_file) = world.get_resource_mut::<SceneFile>() {
                *scene_file = SceneFile::from_path(path.clone());
                scene_file.mark_saved();
            }

            // Count entities for notification
            let count = {
                let mut query = world.query::<&crate::classes::Instance>();
                query.iter(world).count()
            };

            if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
                notifs.success(format!(
                    "Saved {} entities to {}",
                    count,
                    path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string())
                ));
            }

            info!("✅ Scene saved: {:?} ({} entities)", path, count);
        }
        Err(e) => {
            error!("❌ Failed to save scene: {}", e);
            if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
                notifs.error(format!("Failed to save: {}", e));
            }
        }
    }
}

// ============================================================================
// 4. Open Scene — prompt file picker, then load
// ============================================================================

/// Prompt the user for a file, then load it.
fn do_open_scene(world: &mut World) {
    // Show native file picker (blocking)
    let path = pick_open_file();

    if let Some(path) = path {
        do_open_scene_path(world, &path);
    } else {
        info!("📂 Open cancelled by user");
    }
}

/// Load a scene from a specific path.
/// Detects format by magic bytes: binary (starts with "EUSTRESS") vs legacy RON/text.
fn do_open_scene_path(world: &mut World, path: &PathBuf) {
    if !path.exists() {
        error!("❌ Scene file not found: {:?}", path);
        if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
            notifs.error(format!("File not found: {}", path.display()));
        }
        return;
    }

    // Detect format by reading first 8 bytes
    let is_binary = match std::fs::File::open(path) {
        Ok(mut f) => {
            use std::io::Read;
            let mut magic = [0u8; 8];
            f.read_exact(&mut magic).ok();
            &magic == b"EUSTRESS"
        }
        Err(_) => false,
    };

    info!("📂 Opening scene: {:?} (format: {})", path, if is_binary { "binary" } else { "legacy text" });

    // Clear existing Instance entities before loading
    let existing: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<crate::classes::Instance>>();
        query.iter(world).collect()
    };

    let cleared = existing.len();
    for entity in existing {
        world.despawn(entity);
    }
    info!("🗑️ Cleared {} existing entities", cleared);

    if is_binary {
        // Load binary scene into world
        match crate::serialization::load_binary_scene_to_world(world, path) {
            Ok(count) => {
                update_scene_file_after_open(world, path, count);
            }
            Err(e) => {
                error!("❌ Failed to open binary scene: {}", e);
                if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
                    notifs.error(format!("Failed to open: {}", e));
                }
            }
        }
    } else {
        // Legacy text format (RON or JSON) — not directly loadable into World.
        // The user should use --scene CLI flag for legacy files, or re-export to binary.
        warn!("⚠️ Legacy text scene detected: {:?}. Use File > Import or --scene CLI flag.", path);
        if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
            notifs.warning(
                "Legacy RON/JSON scene detected. Please re-export to binary format, \
                 or launch with --scene flag to load at startup."
            );
        }
    }
}

/// Update SceneFile resource and show success notification after opening.
fn update_scene_file_after_open(world: &mut World, path: &PathBuf, count: usize) {
    if let Some(mut scene_file) = world.get_resource_mut::<SceneFile>() {
        *scene_file = SceneFile::from_path(path.clone());
    }

    let display_name = path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());

    if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
        notifs.success(format!("Opened {} entities from {}", count, display_name));
    }

    info!("✅ Scene opened: {:?} ({} entities)", path, count);
}

// ============================================================================
// 5. New Scene — clear world and spawn fresh defaults
// ============================================================================

/// Clear the current scene and spawn a fresh default scene.
fn do_new_scene(world: &mut World) {
    // Clear all Instance entities
    let existing: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<crate::classes::Instance>>();
        query.iter(world).collect()
    };

    let cleared = existing.len();
    for entity in existing {
        world.despawn(entity);
    }

    // Reset SceneFile to untitled
    if let Some(mut scene_file) = world.get_resource_mut::<SceneFile>() {
        *scene_file = SceneFile::new_untitled();
    }

    if let Some(mut notifs) = world.get_resource_mut::<NotificationManager>() {
        notifs.info(format!("New scene created (cleared {} entities)", cleared));
    }

    info!("🆕 New scene: cleared {} entities. Default scene will spawn on next frame via DefaultScenePlugin.", cleared);

    // Note: The camera, sky, atmosphere, and baseplate are spawned by
    // DefaultScenePlugin::setup_default_scene on Startup. For a runtime
    // "New Scene" we'd need to re-trigger that. For now, the user gets
    // an empty scene — they can use the Toolbox to add parts.
}
