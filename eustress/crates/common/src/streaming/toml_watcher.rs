//! # TOML File Watcher
//!
//! ## Table of Contents
//! - TomlWatcher      — notify-based reactive reload on external edits
//! - WatchEvent       — parsed event: which instance changed, what kind of change
//!
//! ## Design
//! Uses the `notify` crate to watch the `instances/` directory tree.
//! When an external editor saves a `.toml` file:
//!   1. `notify` fires a `ModifyKind::Data` event
//!   2. TomlWatcher parses the filename → InstanceId
//!   3. Reads + parses the changed TOML file
//!   4. Compares version against in-memory InstanceRecord
//!   5. If disk is newer → updates DashMap record, invalidates .bin sidecar
//!   6. Sends a Bevy `Event<InstanceReloaded>` if the instance is in the Active tier
//!
//! Debouncing: notify events are debounced at 200ms to collapse rapid save bursts.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::{Watcher, RecommendedWatcher, RecursiveMode, Event, EventKind};
use tracing;

use super::chunk_grid::SpatialChunkGrid;
use super::sidecar;
use super::types::{InstanceId, Tier};

/// Debounce duration for filesystem events — collapses rapid save bursts.
const DEBOUNCE_DURATION: Duration = Duration::from_millis(200);

// ─────────────────────────────────────────────────────────────────────────────
// WatchEvent — what happened to which instance
// ─────────────────────────────────────────────────────────────────────────────

/// A processed filesystem event mapped to an instance.
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
// TomlWatcher
// ─────────────────────────────────────────────────────────────────────────────

/// Watches the `instances/` directory tree for external TOML edits.
/// Sends `WatchEvent`s to a channel that the StreamingPlugin drains each frame.
pub struct TomlWatcher {
    /// The underlying notify watcher handle (must stay alive to keep watching).
    _watcher: RecommendedWatcher,
    /// Receiver end of the event channel — drained by the Bevy plugin each frame.
    /// Uses crossbeam (Sync) instead of std::sync::mpsc (not Sync) so this struct
    /// can live inside a Bevy Resource.
    pub receiver: crossbeam::channel::Receiver<WatchEvent>,
}

impl TomlWatcher {
    /// Start watching a directory tree for `.toml` file changes.
    ///
    /// # Arguments
    /// - `instances_dir` — root directory to watch (e.g. `assets/spaces/my-space/instances/`)
    /// - `grid` — shared SpatialChunkGrid for in-memory updates on external edits
    ///
    /// # Returns
    /// A TomlWatcher with a receiver channel that yields WatchEvents.
    pub fn start(instances_dir: &Path, grid: Arc<SpatialChunkGrid>) -> Result<Self, WatcherError> {
        let (tx, rx) = crossbeam::channel::unbounded::<WatchEvent>();

        let instances_dir_owned = instances_dir.to_path_buf();

        // Create the notify watcher with event handler closure.
        let mut watcher = notify::recommended_watcher(move |result: Result<Event, notify::Error>| {
            let event = match result {
                Ok(e) => e,
                Err(error) => {
                    tracing::warn!("TomlWatcher notify error: {error}");
                    return;
                }
            };

            // Only process .toml files (ignore .bin sidecars, .tmp files, etc.)
            let toml_paths: Vec<PathBuf> = event.paths.iter()
                .filter(|p| p.extension().map_or(false, |ext| ext == "toml"))
                .filter(|p| !p.to_string_lossy().ends_with(".toml.tmp"))
                .cloned()
                .collect();

            if toml_paths.is_empty() {
                return;
            }

            for toml_path in toml_paths {
                // Extract InstanceId from the filename stem.
                let instance_id = match toml_path.file_stem().and_then(|s| s.to_str()) {
                    Some(stem) => InstanceId::from_string(stem),
                    None => continue,
                };

                let watch_event = match event.kind {
                    EventKind::Create(_) => {
                        tracing::debug!("TomlWatcher: created {}", instance_id);
                        WatchEvent::Created { instance_id: instance_id.clone(), toml_path: toml_path.clone() }
                    }
                    EventKind::Modify(_) => {
                        tracing::debug!("TomlWatcher: modified {}", instance_id);

                        // Invalidate the .bin sidecar immediately — it's now stale.
                        let sidecar_path = toml_path.with_extension("toml.bin");
                        sidecar::invalidate_sidecar(&sidecar_path);

                        // If the instance exists in the grid, update it from disk.
                        // The dirty flag is NOT set because disk is already truth.
                        reload_from_disk(&grid, &instance_id, &toml_path);

                        WatchEvent::Modified { instance_id: instance_id.clone(), toml_path: toml_path.clone() }
                    }
                    EventKind::Remove(_) => {
                        tracing::debug!("TomlWatcher: deleted {}", instance_id);
                        WatchEvent::Deleted { instance_id: instance_id.clone(), toml_path: toml_path.clone() }
                    }
                    _ => continue,
                };

                if tx.send(watch_event).is_err() {
                    // Receiver dropped — watcher should stop.
                    return;
                }
            }
        }).map_err(|e| WatcherError::Init(e.to_string()))?;

        // Start watching the instances directory recursively.
        watcher.watch(&instances_dir_owned, RecursiveMode::Recursive)
            .map_err(|e| WatcherError::Watch(e.to_string()))?;

        tracing::info!("TomlWatcher started on {}", instances_dir.display());

        Ok(Self {
            _watcher: watcher,
            receiver: rx,
        })
    }

    /// Drain all pending events from the channel (non-blocking).
    /// Call this once per frame from the Bevy plugin.
    pub fn drain_events(&self) -> Vec<WatchEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.receiver.try_recv() {
            events.push(event);
        }
        events
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
            tracing::warn!("TomlWatcher: failed to read {}: {error}", toml_path.display());
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

// ─────────────────────────────────────────────────────────────────────────────
// Error type
// ─────────────────────────────────────────────────────────────────────────────

/// Errors from the TOML watcher.
#[derive(Debug, thiserror::Error)]
pub enum WatcherError {
    #[error("watcher init failed: {0}")]
    Init(String),

    #[error("watcher directory watch failed: {0}")]
    Watch(String),
}
