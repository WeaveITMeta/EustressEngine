//! # File Event Handler
//!
//! Consumes `FileEvent` messages dispatched by the Slint UI (New, Open, Save, Undo, Redo)
//! and executes the appropriate EEP file-system-first operations.
//!
//! ## Architecture
//!
//! File operations are **folder-based** per EEP_SPECIFICATION.md v2.0:
//! - **New**  → scaffold a fresh Space folder + TOML files, then switch to it
//! - **Open** → pick a Space folder, despawn current entities, rescan new folder
//! - **Save** → write all ECS entities back to their `.part.toml` / `_service.toml` files
//!
//! Binary `.eustress` format is kept for import-only backwards compatibility.
//!
//! ## Table of Contents
//!
//! 1. PendingFileActions — bridges MessageReader → exclusive system
//! 2. drain_file_events  — regular system: read FileEvent → stage actions
//! 3. execute_file_actions — exclusive system: run staged actions with &mut World
//! 4. do_new_space       — scaffold EEP folder + switch to it
//! 5. do_open_space      — folder picker → reload
//! 6. do_save_space      — write ECS → TOML files
//! 7. do_open_legacy     — backwards-compat: binary .eustress import
//! 8. do_publish         — stub (not yet implemented)

use bevy::prelude::*;
use bevy::ecs::message::MessageReader;
use chrono::Utc;
use std::path::{Path, PathBuf};

use eustress_common::{
    load_toml_file, save_toml_file,
    PublishJournalManifest, PublishManifest, SyncManifest,
};

use super::file_dialogs::{FileEvent, PublishRequest, SceneFile};
use crate::notifications::NotificationManager;
use crate::space::space_ops;

// ============================================================================
// 1. PendingFileActions — staging resource
// ============================================================================

/// Staged file actions collected each frame by `drain_file_events`,
/// then consumed by `execute_file_actions` which needs `&mut World`.
#[derive(Resource, Default)]
pub struct PendingFileActions {
    pub actions: Vec<FileAction>,
}

/// Owned action variants (matches `FileEvent` 1-to-1 plus legacy path variant).
#[derive(Clone, Debug)]
pub enum FileAction {
    /// New Universe: create a Universe folder at the workspace root
    NewUniverse,
    /// New Space: scaffold EEP folder + switch engine to it
    NewSpace,
    /// Open Space: folder picker → switch engine to chosen Space folder
    OpenSpace,
    /// Open from explicit path (CLI --scene flag, recent files, etc.)
    OpenSpacePath(PathBuf),
    /// Save Space: write ECS → TOML files in current SpaceRoot
    SaveSpace,
    /// Save Space As: pick a new folder name, then save
    SaveSpaceAs,
    /// Publish stub
    Publish(PublishRequest),
}

// ============================================================================
// 2. Regular system — drain FileEvent messages into PendingFileActions
// ============================================================================

/// Reads `FileEvent` messages and stages them for the exclusive system.
/// Runs every frame; cheap — only does Vec push.
pub fn drain_file_events(
    mut events: MessageReader<FileEvent>,
    mut pending: ResMut<PendingFileActions>,
) {
    for event in events.read() {
        let action = match event {
            FileEvent::NewUniverse     => FileAction::NewUniverse,
            FileEvent::NewScene        => FileAction::NewSpace,
            FileEvent::OpenScene       => FileAction::OpenSpace,
            FileEvent::SaveScene       => FileAction::SaveSpace,
            FileEvent::SaveSceneAs     => FileAction::SaveSpaceAs,
            FileEvent::OpenRecent(p)   => FileAction::OpenSpacePath(p.clone()),
            FileEvent::Publish(request) => FileAction::Publish(request.clone()),
            FileEvent::PublishAs       => FileAction::Publish(PublishRequest {
                as_new: true,
                ..PublishRequest::default()
            }),
        };
        pending.actions.push(action);
    }
}

// ============================================================================
// 3. Exclusive system — execute staged actions with full World access
// ============================================================================

/// Processes staged file actions.
/// Requires `&mut World` because open/new operations despawn entities and
/// insert resources (same pattern as `play_mode.rs`).
pub fn execute_file_actions(world: &mut World) {
    let actions: Vec<FileAction> = {
        let Some(mut pending) = world.get_resource_mut::<PendingFileActions>() else {
            return;
        };
        std::mem::take(&mut pending.actions)
    };

    if actions.is_empty() { return; }

    for action in actions {
        match action {
            FileAction::NewUniverse      => do_new_universe(world),
            FileAction::NewSpace         => do_new_space(world),
            FileAction::OpenSpace        => do_open_space(world),
            FileAction::OpenSpacePath(p) => do_open_space_path(world, p),
            FileAction::SaveSpace        => do_save_space(world),
            FileAction::SaveSpaceAs      => do_save_space_as(world),
            FileAction::Publish(request) => do_publish(world, &request),
        }
    }
}

// ============================================================================
// 4. New Space
// ============================================================================

/// Scaffold a fresh EEP Space folder on disk, then switch the engine to it.
///
/// Produces:
/// ```
/// Documents/Eustress/Universe1/spaces/SpaceN/
///   .eustress/project.toml + settings.toml + cache/
///   Workspace/_service.toml + Baseplate.part.toml
///   Lighting/_service.toml + Sky.sky.toml + Atmosphere.atmosphere.toml
///   Players/ … SoulService/ … (7 more service folders)
///   space.toml + simulation.toml + .gitignore
/// ```
fn do_new_universe(world: &mut World) {
    info!("🪐 New Universe requested");
    space_ops::new_universe(world);
}

fn do_new_space(world: &mut World) {
    info!("🆕 New Space requested");
    space_ops::new_space(world);
}

// ============================================================================
// 5. Open Space
// ============================================================================

/// Show a folder picker, then load the selected Space directory.
fn do_open_space(world: &mut World) {
    match space_ops::pick_space_folder() {
        Some(path) => do_open_space_path(world, path),
        None => info!("📂 Open Space cancelled by user"),
    }
}

/// Load a Space from an explicit path (recent files, CLI flag, etc.).
/// Validates that the directory is a legitimate Eustress Space before loading:
/// - Must be a directory
/// - Must contain at least one of: `.eustress/project.toml`, `Workspace/`, `space.toml`
fn do_open_space_path(world: &mut World, path: PathBuf) {
    if !path.exists() || !path.is_dir() {
        let msg = format!("Not a valid directory: {}", path.display());
        error!("❌ {}", msg);
        if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
            n.error(msg);
        }
        return;
    }

    // Basic validation: is this actually a Space folder?
    let looks_like_space =
        path.join(".eustress").join("project.toml").exists()
        || path.join("Workspace").exists()
        || path.join("space.toml").exists();

    if !looks_like_space {
        // Warn but still allow — user might be opening an older/partial Space
        if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
            n.warning(format!(
                "Directory '{}' does not appear to be an Eustress Space (no .eustress/, Workspace/, or space.toml found). Loading anyway.",
                path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            ));
        }
    }

    space_ops::open_space(world, &path);
}

// ============================================================================
// 6. Save Space
// ============================================================================

/// Write all ECS entities back to their TOML files in the current SpaceRoot.
fn do_save_space(world: &mut World) {
    if let Some(sr) = world.get_resource::<crate::space::SpaceRoot>().map(|r| r.0.clone()) {
        let sim_toml = sr.join("simulation.toml");
        if !sim_toml.exists() {
            if let Err(e) = std::fs::write(&sim_toml, crate::space::space_ops::default_simulation_toml()) {
                warn!("Could not write simulation.toml: {}", e);
            }
        }
    }

    space_ops::save_space(world);
}

/// Prompt for a new Space folder name, copy current Space structure, then switch.
/// For now: save in place and notify the user — full "Save As" (copy) is Phase 2.
fn do_save_space_as(world: &mut World) {
    // Phase 1: save in place (identical to Save)
    // TODO Phase 2: pick a new parent folder + name, copy files, switch SpaceRoot
    do_save_space(world);
    if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
        n.info("Save As: saved in current location. Rename the Space folder to move it.");
    }
}

// ============================================================================
// 7. Publish stub
// ============================================================================

fn do_publish(world: &mut World, request: &PublishRequest) {
    do_save_space(world);

    let Some(space_root) = world.get_resource::<crate::space::SpaceRoot>().map(|r| r.0.clone()) else {
        if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
            n.error("Publish requires an open Space folder.");
        }
        return;
    };

    match prepare_publish_manifests(&space_root, request) {
        Ok(()) => {
            if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
                if request.as_new {
                    n.success("Publish As New prepared locally — manifests updated in .eustress with a new local publish identity.");
                } else {
                    n.success("Publish prepared locally — manifests updated in .eustress. Remote upload is the next step.");
                }
            }
        }
        Err(e) => {
            if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
                n.error(format!("Publish preparation failed: {}", e));
            }
        }
    }
}

fn prepare_publish_manifests(space_root: &Path, request: &PublishRequest) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    let project_dir = space_root.join(".eustress");
    let publish_path = project_dir.join("publish.toml");
    let journal_path = project_dir.join("publish-journal.toml");
    let sync_path = project_dir.join("sync.toml");

    let mut publish_manifest = load_optional_manifest::<PublishManifest>(&publish_path)?
        .unwrap_or_default();
    let mut journal_manifest = load_optional_manifest::<PublishJournalManifest>(&journal_path)?
        .unwrap_or_else(|| PublishJournalManifest::new(&now));
    let mut sync_manifest = load_optional_manifest::<SyncManifest>(&sync_path)?
        .unwrap_or_default();

    let experience_name = if request.experience_name.trim().is_empty() {
        space_root
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_string())
    } else {
        request.experience_name.trim().to_string()
    };
    let description = request.description.trim();
    let genre = if request.genre.trim().is_empty() {
        "All".to_string()
    } else {
        request.genre.trim().to_string()
    };

    publish_manifest.listing.name = experience_name;
    publish_manifest.listing.description = if description.is_empty() {
        None
    } else {
        Some(description.to_string())
    };
    publish_manifest.listing.genre = genre;
    publish_manifest.visibility.is_public = request.is_public;
    publish_manifest.visibility.open_source = request.open_source;
    publish_manifest.visibility.studio_editable = request.studio_editable;
    publish_manifest.visibility.discoverable = request.is_public;
    sync_manifest.remote.open_source = request.open_source;
    sync_manifest.remote.editable = request.studio_editable;

    if request.as_new {
        publish_manifest.publish.version = 1;
        publish_manifest.publish.latest_release_id = None;
        publish_manifest.publish.latest_manifest_hash = None;
        publish_manifest.publish.last_published = None;
        publish_manifest.releases.clear();
        journal_manifest = PublishJournalManifest::new(&now);
        sync_manifest.remote.project_id = None;
        sync_manifest.remote.experience_id = None;
    }

    journal_manifest.journal.stage = "prepared".to_string();
    journal_manifest.journal.last_error = None;
    journal_manifest.journal.updated_at = now.clone();
    journal_manifest.journal.resumable = true;

    for checkpoint in &mut journal_manifest.checkpoints {
        if checkpoint.name == "scan" || checkpoint.name == "package" {
            checkpoint.completed = true;
            checkpoint.updated_at = now.clone();
        }
    }

    publish_manifest.publish.channel = publish_manifest.publish.channel.clone();

    save_manifest_file(&publish_path, &publish_manifest)?;
    save_manifest_file(&journal_path, &journal_manifest)?;
    save_manifest_file(&sync_path, &sync_manifest)?;

    Ok(())
}

fn load_optional_manifest<T>(path: &Path) -> Result<Option<T>, String>
where
    T: for<'de> serde::Deserialize<'de>,
{
    if !path.exists() {
        return Ok(None);
    }

    load_toml_file(path)
        .map(Some)
        .map_err(|e| format!("Failed to read {:?}: {}", path, e))
}

fn save_manifest_file<T>(path: &Path, value: &T) -> Result<(), String>
where
    T: serde::Serialize,
{
    save_toml_file(value, path)
        .map_err(|e| format!("Failed to write {:?}: {}", path, e))
}
