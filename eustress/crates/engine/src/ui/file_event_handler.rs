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

    // Provide feedback
    if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
        n.success("Space saved. All TOML files up to date.");
    }
}

/// Prompt for a new Space folder name + parent, copy the current Space
/// directory to that location, then switch `SpaceRoot` so all subsequent
/// saves/loads target the new folder. The file watcher is paused during the
/// copy to avoid phantom delete/create events.
fn do_save_space_as(world: &mut World) {
    // 1. Commit everything in-memory to disk at the current location first.
    do_save_space(world);

    // 2. Capture the current SpaceRoot so we have a source to copy from.
    let Some(src_root) = world.get_resource::<crate::space::SpaceRoot>().map(|r| r.0.clone()) else {
        if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
            n.error("Save As: no active Space to copy from.");
        }
        return;
    };

    // 3. Prompt for a destination folder. The picker returns the *parent*
    //    directory; the new Space keeps the source's folder name (user can
    //    rename after via Explorer if desired).
    let default_name = src_root.file_name().and_then(|n| n.to_str()).unwrap_or("Space").to_string();
    let picked = rfd::FileDialog::new()
        .set_title("Save Space As — pick parent folder")
        .pick_folder();
    let Some(parent) = picked else {
        if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
            n.info("Save As cancelled.");
        }
        return;
    };
    let dst_root = parent.join(&default_name);

    // 4. Refuse to overwrite an existing folder — this avoids silently merging
    //    into another Space and corrupting both.
    if dst_root.exists() {
        if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
            n.error(format!("Save As: '{}' already exists. Pick a different parent or rename first.", dst_root.display()));
        }
        return;
    }

    // 5. Pause the file watcher so the copy doesn't generate spurious events.
    if let Some(mut registry) = world.get_resource_mut::<crate::space::file_loader::SpaceFileRegistry>() {
        registry.rename_in_progress.insert(src_root.clone());
    }

    // 6. Recursive directory copy.
    let copy_result = copy_dir_recursive(&src_root, &dst_root);

    if let Some(mut registry) = world.get_resource_mut::<crate::space::file_loader::SpaceFileRegistry>() {
        registry.rename_in_progress.remove(&src_root);
    }

    match copy_result {
        Ok(()) => {
            // 7. Switch SpaceRoot. File loaders watching the new root will
            //    re-bind on next scan; open tabs keep their in-memory state.
            if let Some(mut sr) = world.get_resource_mut::<crate::space::SpaceRoot>() {
                sr.0 = dst_root.clone();
            }
            if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
                n.success(format!("Saved Space as '{}'", dst_root.display()));
            }
        }
        Err(e) => {
            if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
                n.error(format!("Save As failed: {}", e));
            }
        }
    }
}

/// Recursively copy `src` to `dst`. Creates `dst` if missing.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let name = entry.file_name();
        let src_path = entry.path();
        let dst_path = dst.join(&name);
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if ty.is_file() {
            std::fs::copy(&src_path, &dst_path)?;
        }
        // Skip symlinks — safer default for Space folders.
    }
    Ok(())
}

// ============================================================================
// 7. Publish
// ============================================================================

/// Tracks publish progress for UI display.
#[derive(Resource, Default, Clone)]
pub struct PublishProgress {
    pub stage: String,
    pub percent: f32,
    pub error: Option<String>,
    pub complete: bool,
}

fn do_publish(world: &mut World, request: &PublishRequest) {
    do_save_space(world);

    let Some(space_root) = world.get_resource::<crate::space::SpaceRoot>().map(|r| r.0.clone()) else {
        if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
            n.error("Publish requires an open Space folder.");
        }
        return;
    };

    // Resolve the Universe root (parent of the Space)
    let universe_root = crate::space::universe_root_for_path(&space_root)
        .unwrap_or_else(|| space_root.clone());

    let auth_token = world.get_resource::<crate::auth::AuthState>()
        .and_then(|a| a.token.clone());

    let Some(token) = auth_token else {
        if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
            n.error("Sign in to publish.");
        }
        return;
    };

    // Prepare local manifests
    if let Err(e) = prepare_publish_manifests(&space_root, request) {
        if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
            n.error(format!("Publish preparation failed: {}", e));
        }
        return;
    }

    // Auto-capture thumbnail from viewport if none exists
    capture_thumbnail_from_viewport(world, &universe_root);

    // Setup progress tracking
    let progress = std::sync::Arc::new(std::sync::Mutex::new(PublishProgress {
        stage: "Packaging...".to_string(),
        percent: 0.0,
        error: None,
        complete: false,
    }));

    let progress_for_thread = progress.clone();
    world.insert_resource(PublishProgressHandle(progress));

    // Package + upload in a background thread
    let request = request.clone();
    let is_space_only = request.space_only;
    let space_root_clone = space_root.clone();
    std::thread::spawn(move || {
        let result = if request.space_only {
            execute_space_upload(&space_root_clone, &universe_root, &request, &token, &progress_for_thread)
        } else {
            execute_publish_upload(&universe_root, &request, &token, &progress_for_thread)
        };
        match result {
            Ok(sim_id) => {
                let mut p = progress_for_thread.lock().unwrap();
                p.stage = format!("Published: {}", sim_id);
                p.percent = 100.0;
                p.complete = true;
                tracing::info!("Published successfully: {}", sim_id);
            }
            Err(e) => {
                let mut p = progress_for_thread.lock().unwrap();
                p.stage = "Failed".to_string();
                p.error = Some(e.clone());
                p.complete = true;
                tracing::error!("Publish failed: {}", e);
            }
        }
    });

    if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
        if is_space_only {
            n.info("Publishing Space... packaging and uploading.");
        } else {
            n.info("Publishing Universe... packaging all Spaces and uploading.");
        }
    }
}

/// Resource holding the Arc to the publish progress (for UI polling).
#[derive(Resource)]
struct PublishProgressHandle(std::sync::Arc<std::sync::Mutex<PublishProgress>>);

/// Capture a thumbnail from the current viewport and save to .eustress/thumbnail.png.
/// Spawns a Screenshot entity — Bevy captures the primary window next frame,
/// then our observer resizes to 512x288 and saves to disk.
fn capture_thumbnail_from_viewport(world: &mut World, universe_root: &std::path::Path) {
    let thumb_path = universe_root.join(".eustress").join("thumbnail.png");

    // If a thumbnail already exists and is recent (< 5 min), skip capture
    if thumb_path.exists() {
        if let Ok(meta) = std::fs::metadata(&thumb_path) {
            if let Ok(modified) = meta.modified() {
                if modified.elapsed().unwrap_or_default().as_secs() < 300 {
                    tracing::info!("Recent thumbnail exists, skipping capture");
                    return;
                }
            }
        }
    }

    let _ = std::fs::create_dir_all(universe_root.join(".eustress"));

    // Spawn a Screenshot entity targeting the primary window.
    // The observer fires when the GPU readback completes (next frame).
    let save_path = thumb_path.clone();
    world.spawn(bevy::render::view::screenshot::Screenshot::primary_window())
        .observe(move |trigger: bevy::ecs::observer::On<bevy::render::view::screenshot::ScreenshotCaptured>| {
            let img = trigger.image.clone();
            let path = save_path.clone();
            // Resize + save on the async compute pool to avoid blocking render
            bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                match img.try_into_dynamic() {
                    Ok(dyn_img) => {
                        let rgb = dyn_img.to_rgb8();
                        let thumb = image::imageops::resize(&rgb, 512, 288, image::imageops::FilterType::Lanczos3);
                        match thumb.save(&path) {
                            Ok(_) => tracing::info!("Thumbnail captured: {:?} (512x288)", path),
                            Err(e) => tracing::warn!("Failed to save thumbnail: {}", e),
                        }
                    }
                    Err(e) => tracing::warn!("Thumbnail image conversion failed: {}", e),
                }
            }).detach();
        });

    tracing::info!("Screenshot queued for next frame → {:?}", thumb_path);
}

/// Auto-save system — saves Space to disk every 60 seconds during editing mode.
/// Only runs when not in play mode to avoid saving simulation state.
pub fn auto_save_system(
    space_root: Option<Res<crate::space::SpaceRoot>>,
    play_mode: Option<Res<State<crate::play_mode::PlayModeState>>>,
    mut last_save: Local<Option<std::time::Instant>>,
    mut output: Option<ResMut<super::slint_ui::OutputConsole>>,
    mut file_events: bevy::ecs::message::MessageWriter<super::FileEvent>,
) {
    // Only auto-save in editing mode
    if let Some(ref pms) = play_mode {
        if *pms.get() != crate::play_mode::PlayModeState::Editing {
            return;
        }
    }
    if space_root.is_none() { return; }

    let now = std::time::Instant::now();
    let interval = std::time::Duration::from_secs(60);

    if let Some(last) = *last_save {
        if now.duration_since(last) < interval {
            return;
        }
    } else {
        // First frame — set timer but don't save yet
        *last_save = Some(now);
        return;
    }

    *last_save = Some(now);

    // Auto-save is non-exclusive, so we can't call save_space(&mut World) here.
    // Fire a SaveScene event — `drain_file_events` + `execute_file_actions`
    // will pick it up and perform the exclusive save on the next tick.
    file_events.write(super::FileEvent::SaveScene);
    if let Some(ref mut out) = output {
        out.info("Auto-save triggered.".to_string());
    }
}

const PUBLISH_API: &str = "https://api.eustress.dev";

type ProgressHandle = std::sync::Arc<std::sync::Mutex<PublishProgress>>;

fn set_progress(handle: &ProgressHandle, stage: &str, percent: f32) {
    if let Ok(mut p) = handle.lock() {
        p.stage = stage.to_string();
        p.percent = percent;
    }
}

/// Package the Universe into a .pak and upload to the API.
/// Runs on a background thread. Returns the simulation ID on success.
fn execute_publish_upload(
    universe_root: &std::path::Path,
    request: &PublishRequest,
    token: &str,
    progress: &ProgressHandle,
) -> Result<String, String> {
    // Step 1: Package the entire Universe into a .pak (tar + zstd)
    set_progress(progress, "Packaging Universe...", 5.0);
    let pak_bytes = package_universe_to_pak(universe_root)?;
    let pak_size_mb = pak_bytes.len() as f64 / 1_048_576.0;
    tracing::info!("Packaged {:.1} MB .pak", pak_size_mb);

    // Step 1b: Check hash against last published — skip upload if unchanged
    let pak_hash = blake3::hash(&pak_bytes).to_hex().to_string();
    let hash_path = universe_root.join(".eustress").join(".last_publish_hash");
    if let Ok(last_hash) = std::fs::read_to_string(&hash_path) {
        if last_hash.trim() == pak_hash {
            tracing::info!("Universe unchanged since last publish (hash match), skipping upload");
            set_progress(progress, "No changes to publish", 100.0);
            return Err("No changes since last publish".to_string());
        }
    }

    set_progress(progress, "Creating listing...", 15.0);

    // Step 2: Create listing via POST /api/simulations/publish
    let experience_name = if request.experience_name.trim().is_empty() {
        universe_root.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_string())
    } else {
        request.experience_name.clone()
    };

    let publish_body = serde_json::json!({
        "name": experience_name,
        "description": request.description,
        "genre": request.genre,
        "max_players": 10,
    });

    let resp = ureq::post(&format!("{}/api/simulations/publish", PUBLISH_API))
        .set("Authorization", &format!("Bearer {}", token))
        .set("Content-Type", "application/json")
        .send_string(&publish_body.to_string())
        .map_err(|e| format!("Create listing failed: {}", e))?;

    let resp_body: serde_json::Value = resp.into_json()
        .map_err(|e| format!("Parse listing response: {}", e))?;

    let sim_id = resp_body["id"].as_str()
        .ok_or("Missing simulation ID in response")?
        .to_string();

    tracing::info!("Created listing: {}", sim_id);
    set_progress(progress, "Uploading Universe...", 25.0);

    // Step 3: Upload .pak — single PUT for <100MB, multipart for larger
    const MULTIPART_THRESHOLD: usize = 100 * 1024 * 1024; // 100MB
    const CHUNK_SIZE: usize = 95 * 1024 * 1024; // 95MB per part (under Worker 100MB limit)

    if pak_bytes.len() < MULTIPART_THRESHOLD {
        // Single PUT upload
        ureq::put(&format!("{}/api/simulations/{}/space", PUBLISH_API, sim_id))
            .set("Authorization", &format!("Bearer {}", token))
            .set("Content-Type", "application/octet-stream")
            .send_bytes(&pak_bytes)
            .map_err(|e| format!("Space upload failed: {}", e))?;
        tracing::info!("Universe .pak uploaded (single PUT)");
    } else {
        // Multipart upload for large Universes
        tracing::info!("Large .pak ({:.1} MB) — using multipart upload", pak_bytes.len() as f64 / 1_048_576.0);

        // Create multipart upload
        let create_resp = ureq::post(&format!("{}/api/simulations/{}/space/multipart/create", PUBLISH_API, sim_id))
            .set("Authorization", &format!("Bearer {}", token))
            .call()
            .map_err(|e| format!("Multipart create failed: {}", e))?;
        let create_body: serde_json::Value = create_resp.into_json()
            .map_err(|e| format!("Parse multipart create: {}", e))?;
        let upload_id = create_body["upload_id"].as_str()
            .ok_or("Missing upload_id")?.to_string();

        // Upload chunks
        let mut parts: Vec<serde_json::Value> = Vec::new();
        let total_parts = (pak_bytes.len() + CHUNK_SIZE - 1) / CHUNK_SIZE;

        for (i, chunk) in pak_bytes.chunks(CHUNK_SIZE).enumerate() {
            let part_number = i + 1;
            let pct = 30.0 + (part_number as f32 / total_parts as f32) * 55.0;
            set_progress(progress, &format!("Uploading part {}/{}...", part_number, total_parts), pct);
            tracing::info!("Uploading part {}/{} ({:.1} MB)", part_number, total_parts, chunk.len() as f64 / 1_048_576.0);

            let part_resp = ureq::put(&format!(
                "{}/api/simulations/{}/space/multipart/part?upload_id={}&part_number={}",
                PUBLISH_API, sim_id, upload_id, part_number
            ))
                .set("Authorization", &format!("Bearer {}", token))
                .set("Content-Type", "application/octet-stream")
                .send_bytes(chunk)
                .map_err(|e| format!("Part {} upload failed: {}", part_number, e))?;

            let part_body: serde_json::Value = part_resp.into_json()
                .map_err(|e| format!("Parse part {} response: {}", part_number, e))?;

            parts.push(serde_json::json!({
                "part_number": part_number,
                "etag": part_body["etag"].as_str().unwrap_or(""),
            }));
        }

        // Complete multipart upload
        let complete_body = serde_json::json!({
            "upload_id": upload_id,
            "parts": parts,
            "total_size": pak_bytes.len(),
        });
        ureq::post(&format!("{}/api/simulations/{}/space/multipart/complete", PUBLISH_API, sim_id))
            .set("Authorization", &format!("Bearer {}", token))
            .set("Content-Type", "application/json")
            .send_string(&complete_body.to_string())
            .map_err(|e| format!("Multipart complete failed: {}", e))?;

        tracing::info!("Universe .pak uploaded (multipart, {} parts)", parts.len());
    }

    set_progress(progress, "Uploading thumbnail...", 90.0);

    // Step 4: Upload thumbnail if available (check .eustress/ in Universe root)
    for ext in &["png", "webp", "jpg"] {
        let thumb_path = universe_root.join(".eustress").join(format!("thumbnail.{}", ext));
        if thumb_path.exists() {
            if let Ok(thumb_bytes) = std::fs::read(&thumb_path) {
                let content_type = match *ext {
                    "png" => "image/png",
                    "jpg" => "image/jpeg",
                    _ => "image/webp",
                };
                let _ = ureq::put(&format!("{}/api/simulations/{}/thumbnail", PUBLISH_API, sim_id))
                    .set("Authorization", &format!("Bearer {}", token))
                    .set("Content-Type", content_type)
                    .send_bytes(&thumb_bytes);
                tracing::info!("Thumbnail uploaded ({})", ext);
                break;
            }
        }
    }

    // Save hash for duplicate detection on next publish
    let _ = std::fs::create_dir_all(universe_root.join(".eustress"));
    let _ = std::fs::write(&hash_path, &pak_hash);

    set_progress(progress, "Complete", 100.0);
    Ok(sim_id)
}

/// Publish a single Space incrementally to an already-published Universe.
/// Packages just the Space folder and uploads via PUT /api/simulations/{id}/spaces/{name}.
fn execute_space_upload(
    space_root: &std::path::Path,
    universe_root: &std::path::Path,
    request: &PublishRequest,
    token: &str,
    progress: &ProgressHandle,
) -> Result<String, String> {
    // Get the experience_id from sync.toml
    let sync_path = universe_root.join(".eustress").join("sync.toml");
    let sync_manifest = eustress_common::load_toml_file::<eustress_common::SyncManifest>(&sync_path)
        .map_err(|e| format!("Load sync.toml: {}", e))?;
    let sim_id = sync_manifest.remote.experience_id
        .ok_or("Universe not published yet — publish the Universe first")?;

    let space_name = space_root.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "default".to_string());

    set_progress(progress, &format!("Packaging Space '{}'...", space_name), 10.0);

    // Package just this Space folder
    let pak_bytes = package_universe_to_pak(space_root)?;
    let pak_size_mb = pak_bytes.len() as f64 / 1_048_576.0;
    tracing::info!("Packaged Space '{}': {:.1} MB", space_name, pak_size_mb);

    set_progress(progress, &format!("Uploading Space '{}'...", space_name), 40.0);

    // Upload to PUT /api/simulations/{id}/spaces/{name}
    let encoded_name = urlencoding::encode(&space_name);
    ureq::put(&format!("{}/api/simulations/{}/spaces/{}", PUBLISH_API, sim_id, encoded_name))
        .set("Authorization", &format!("Bearer {}", token))
        .set("Content-Type", "application/octet-stream")
        .send_bytes(&pak_bytes)
        .map_err(|e| format!("Space upload failed: {}", e))?;

    set_progress(progress, "Complete", 100.0);
    tracing::info!("Space '{}' published to Universe {}", space_name, sim_id);
    Ok(sim_id)
}

/// Package an entire Universe folder into a zstd-compressed tar archive (.pak).
///
/// Includes:
/// - All Spaces (spaces/Space1/, spaces/Space2/, ...)
/// - universe.toml
/// - knowledge/ folder (recordings, training data)
/// - .eustress/ metadata (publish manifests, config)
///
/// Excludes:
/// - .git/ (version control)
/// - node_modules/
/// - target/ (build artifacts)
/// - Temporary/lock files
fn package_universe_to_pak(universe_root: &std::path::Path) -> Result<Vec<u8>, String> {
    let mut tar_bytes = Vec::new();
    {
        let mut tar_builder = tar::Builder::new(&mut tar_bytes);

        fn walk_dir(
            builder: &mut tar::Builder<&mut Vec<u8>>,
            dir: &std::path::Path,
            base: &std::path::Path,
        ) -> Result<(), String> {
            let entries = std::fs::read_dir(dir)
                .map_err(|e| format!("Read dir {:?}: {}", dir, e))?;

            for entry in entries {
                let entry = entry.map_err(|e| format!("Dir entry: {}", e))?;
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();

                // Skip version control, build artifacts, temp files
                if name == ".git" || name == "node_modules" || name == "target" {
                    continue;
                }
                // Skip OS junk
                if name == ".DS_Store" || name == "Thumbs.db" || name == "desktop.ini" {
                    continue;
                }
                // Skip lock files
                if name.ends_with(".lock") || name.ends_with(".tmp") {
                    continue;
                }

                let rel = path.strip_prefix(base).unwrap_or(&path);

                if path.is_dir() {
                    walk_dir(builder, &path, base)?;
                } else {
                    builder.append_path_with_name(&path, rel)
                        .map_err(|e| format!("Add {:?}: {}", rel, e))?;
                }
            }
            Ok(())
        }

        walk_dir(&mut tar_builder, universe_root, universe_root)?;
        tar_builder.finish().map_err(|e| format!("Finalize tar: {}", e))?;
    }

    // Compress with zstd (level 3 — good balance of speed and size)
    let compressed = zstd::encode_all(std::io::Cursor::new(&tar_bytes), 3)
        .map_err(|e| format!("Zstd compress: {}", e))?;

    Ok(compressed)
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

    // Reset manifests on first Universe publish (no existing experience_id)
    let is_first_publish = sync_manifest.remote.experience_id.is_none();
    if is_first_publish {
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
