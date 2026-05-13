//! # Streaming TOML reload — consumer of the unified file-change feed.
//!
//! ## Architecture (post-2026-05-12 consolidation)
//!
//! This module used to own its own `notify::RecommendedWatcher`,
//! watching the same `Workspace/` tree as the engine's
//! `SpaceFileWatcher`. Two notify watchers reading the same files
//! under Windows share-mode rules raced on every save — the engine's
//! write would fail with `os error 32`, the user's edit would
//! silently vanish, and copy-paste read pre-edit content from disk.
//!
//! Now there is exactly ONE notify watcher in the workspace:
//! `engine::space::file_watcher::SpaceFileWatcher`. After its own
//! processing pass, it broadcasts
//! [`eustress_common::file_events::FileChanged`] messages. This
//! module subscribes via [`sync_grid_from_file_events`] — a stateless
//! Bevy system, no second watcher, no concurrent reads — and updates
//! the [`SpatialChunkGrid`] from the just-changed `_instance.toml`
//! files.
//!
//! The `WatchEvent` enum is kept as the StreamingPlugin's downstream
//! payload (`InstanceReloaded` / `InstancePromoted` Bevy events read
//! by spatial queries) so existing consumers of those events keep
//! working unchanged.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::chunk_grid::SpatialChunkGrid;
use super::sidecar;
use super::types::InstanceId;

// ─────────────────────────────────────────────────────────────────────────────
// WatchEvent — what happened to which instance (downstream payload)
// ─────────────────────────────────────────────────────────────────────────────

/// A processed filesystem event mapped to an instance. Translated from
/// [`FileChanged`](crate::file_events::FileChanged) by
/// [`sync_grid_from_file_events`] for systems that prefer working with
/// `InstanceId` rather than raw paths (spatial queries, sidecar
/// invalidation, the streaming radius gate).
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// An existing instance's TOML was modified externally.
    Modified {
        instance_id: InstanceId,
        toml_path: PathBuf,
    },
    /// A new TOML file appeared (external create).
    Created {
        instance_id: InstanceId,
        toml_path: PathBuf,
    },
    /// A TOML file was deleted externally.
    Deleted {
        instance_id: InstanceId,
        toml_path: PathBuf,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// Stateless consumer: turn FileChanged messages → SpatialChunkGrid updates
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a [`FileChanged`](crate::file_events::FileChanged) into a
/// [`WatchEvent`] iff the path looks like an instance TOML the
/// streaming grid cares about. Returns `None` for `.tmp` files,
/// non-`.toml` paths, or any path whose stem doesn't parse as an
/// `InstanceId`. Pure — no I/O.
pub fn classify_file_change(
    path: &Path,
    kind: crate::file_events::FileChangeKind,
) -> Option<WatchEvent> {
    // Only `.toml` files participate. `.toml.tmp` files are the
    // atomic-write staging from `gui_loader::write_atomic` and must
    // be skipped — they get renamed onto the real path moments later
    // and that rename produces its own event.
    let path_str = path.to_string_lossy();
    if !path_str.ends_with(".toml") || path_str.ends_with(".toml.tmp") {
        return None;
    }
    let stem = path.file_stem().and_then(|s| s.to_str())?;
    let instance_id = InstanceId::from_string(stem);
    match kind {
        crate::file_events::FileChangeKind::Created => {
            Some(WatchEvent::Created { instance_id, toml_path: path.to_path_buf() })
        }
        crate::file_events::FileChangeKind::Modified => {
            Some(WatchEvent::Modified { instance_id, toml_path: path.to_path_buf() })
        }
        crate::file_events::FileChangeKind::Removed => {
            Some(WatchEvent::Deleted { instance_id, toml_path: path.to_path_buf() })
        }
    }
}

/// Side-effecting handler — invoked from
/// [`sync_grid_from_file_events`] for each TOML-classified event. On
/// Modify we invalidate the `.bin` sidecar and refresh the grid
/// record from disk; on Create/Delete we leave the grid alone (the
/// streaming index rebuild already handles those).
pub fn apply_watch_event(grid: &Arc<SpatialChunkGrid>, event: &WatchEvent) {
    if let WatchEvent::Modified { instance_id, toml_path } = event {
        let sidecar_path = toml_path.with_extension("toml.bin");
        sidecar::invalidate_sidecar(&sidecar_path);
        reload_from_disk(grid, instance_id, toml_path);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Reload helper — parse TOML from disk and update in-memory record
// ─────────────────────────────────────────────────────────────────────────────

/// Re-read a TOML file from disk and update the in-memory InstanceRecord.
/// Does NOT set dirty=true because the disk is already the source of truth.
fn reload_from_disk(grid: &SpatialChunkGrid, id: &InstanceId, toml_path: &Path) {
    let content = match std::fs::read_to_string(toml_path) {
        Ok(c) => c,
        Err(error) => {
            // Now that there's only one watcher in the workspace, a
            // read failure here is real (e.g. external editor deleted
            // the file mid-event) rather than a self-inflicted file
            // lock — downgraded from warn! to debug! so external-edit
            // bursts don't spam the console.
            tracing::debug!("TomlWatcher: failed to read {}: {error}", toml_path.display());
            return;
        }
    };

    let parsed: TomlInstance = match toml::from_str(&content) {
        Ok(p) => p,
        Err(error) => {
            tracing::warn!("TomlWatcher: failed to parse {}: {error}", toml_path.display());
            return;
        }
    };

    // Update the grid record with parsed data.
    grid.update(id, |record| {
        record.bin.position = [
            parsed.position.first().copied().unwrap_or(0.0),
            parsed.position.get(1).copied().unwrap_or(0.0),
            parsed.position.get(2).copied().unwrap_or(0.0),
        ];
        record.bin.rotation = [
            parsed.rotation.first().copied().unwrap_or(0.0),
            parsed.rotation.get(1).copied().unwrap_or(0.0),
            parsed.rotation.get(2).copied().unwrap_or(0.0),
        ];
        record.bin.scale = parsed.scale;
        record.bin.class_id = parsed.class_id;
        record.bin.velocity = parsed.velocity;
        record.name = parsed.name;
        record.tags = parsed.tags;
        // NOT setting dirty — disk is already truth.
    });
}

/// TOML-deserializable instance shape for reload.
#[derive(serde::Deserialize)]
struct TomlInstance {
    #[serde(default)]
    name:     String,
    #[serde(default)]
    tags:     Vec<String>,
    #[serde(default)]
    position: Vec<f32>,
    #[serde(default)]
    rotation: Vec<f32>,
    #[serde(default = "default_scale")]
    scale:    f32,
    #[serde(default)]
    class_id: u32,
    #[serde(default)]
    velocity: f32,
}

fn default_scale() -> f32 { 1.0 }
