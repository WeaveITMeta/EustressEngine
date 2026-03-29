//! # UniverseRegistry
//!
//! Periodic scanner that maintains the list of all Universes and their Spaces
//! found under `Documents/Eustress/`.  The registry is a Bevy Resource updated
//! on Startup and then every 5 seconds from an Update system.
//!
//! ## Types
//! - `SpaceInfo`         — name + path for one Space
//! - `UniverseInfo`      — name + path + Vec<SpaceInfo> for one Universe
//! - `UniverseRegistry`  — Vec<UniverseInfo> + active_space; Bevy Resource
//! - `UniverseRegistryPlugin` — registers resource + scan systems

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use crossbeam_channel::{unbounded, Receiver};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, FileIdMap};

use super::{looks_like_space_root, workspace_root, SpaceRoot};

// ─────────────────────────────────────────────────────────────────────────────
// Data types
// ─────────────────────────────────────────────────────────────────────────────

/// Minimal info about one Space folder.
#[derive(Debug, Clone)]
pub struct SpaceInfo {
    pub path: PathBuf,
    pub name: String,
}

/// Minimal info about one Universe folder and its contained Spaces.
#[derive(Debug, Clone)]
pub struct UniverseInfo {
    pub path: PathBuf,
    pub name: String,
    pub spaces: Vec<SpaceInfo>,
}

// ─────────────────────────────────────────────────────────────────────────────
// UniverseRegistry
// ─────────────────────────────────────────────────────────────────────────────

const RESCAN_INTERVAL: Duration = Duration::from_secs(5);

/// Bevy Resource — keeps the full Universe→Space tree for the sidebar.
#[derive(Resource, Default, Debug)]
pub struct UniverseRegistry {
    pub universes: Vec<UniverseInfo>,
    /// Path of the currently active Space (mirrors `SpaceRoot`).
    pub active_space: Option<PathBuf>,
    last_scan: Option<Instant>,
    /// Set by the notify watcher to force an immediate rescan.
    pub rescan_requested: bool,
}

impl UniverseRegistry {
    /// Scan `Documents/Eustress/` and rebuild the universe list.
    pub fn scan(&mut self) {
        let workspace = workspace_root();
        let mut universes = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&workspace) {
            let mut dirs: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir() && !looks_like_space_root(p))
                .collect();
            dirs.sort();

            for universe_path in dirs {
                let name = universe_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                let spaces = collect_spaces(&universe_path);
                universes.push(UniverseInfo { path: universe_path, name, spaces });
            }
        }

        self.universes = universes;
        self.last_scan = Some(Instant::now());
        self.rescan_requested = false;
    }

    fn needs_rescan(&self) -> bool {
        self.rescan_requested
            || self.last_scan.map(|t| t.elapsed() >= RESCAN_INTERVAL).unwrap_or(true)
    }

    /// Find the `UniverseInfo` that contains `space_path`.
    pub fn universe_for_space(&self, space_path: &Path) -> Option<&UniverseInfo> {
        self.universes.iter().find(|u| {
            u.spaces.iter().any(|s| s.path == space_path)
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// UniverseWatcher
// ─────────────────────────────────────────────────────────────────────────────

/// Bevy non-send resource — watches `Documents/Eustress/` for new/removed
/// `space.toml` and `project.toml` files, then sets `registry.rescan_requested`.
#[derive(Resource)]
pub struct UniverseWatcher {
    _debouncer: Debouncer<RecommendedWatcher, FileIdMap>,
    receiver: Receiver<DebounceEventResult>,
}

impl UniverseWatcher {
    pub fn new(workspace: &Path) -> Result<Self, String> {
        let (tx, rx) = unbounded();

        let mut debouncer = new_debouncer(
            Duration::from_millis(500),
            None,
            move |result: DebounceEventResult| {
                let _ = tx.send(result);
            },
        )
        .map_err(|e| format!("UniverseWatcher: {e}"))?;

        debouncer
            .watcher()
            .watch(workspace, RecursiveMode::Recursive)
            .map_err(|e| format!("UniverseWatcher watch: {e}"))?;

        info!("UniverseWatcher: watching {:?}", workspace);
        Ok(Self { _debouncer: debouncer, receiver: rx })
    }

    /// Drain pending events; return `true` if any relevant file was created/removed.
    pub fn poll(&self) -> bool {
        let mut relevant = false;
        while let Ok(result) = self.receiver.try_recv() {
            if let Ok(events) = result {
                for event in events {
                    if is_space_marker(&event.event) {
                        relevant = true;
                    }
                }
            }
        }
        relevant
    }
}

fn is_space_marker(event: &notify::Event) -> bool {
    use notify::EventKind;
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(_)
    ) && event.paths.iter().any(|p| {
        p.file_name()
            .map(|n| n == "space.toml" || n == "project.toml")
            .unwrap_or(false)
    })
}

fn collect_spaces(universe_path: &Path) -> Vec<SpaceInfo> {
    let spaces_dir = universe_path.join("spaces");
    let search_in = if spaces_dir.is_dir() { spaces_dir } else { universe_path.to_path_buf() };

    let Ok(entries) = std::fs::read_dir(&search_in) else { return Vec::new() };

    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && looks_like_space_root(p))
        .collect();
    dirs.sort();

    dirs.into_iter().map(|path| {
        let name = path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        SpaceInfo { path, name }
    }).collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Bevy systems
// ─────────────────────────────────────────────────────────────────────────────

fn startup_scan(
    mut registry: ResMut<UniverseRegistry>,
    space_root: Option<Res<SpaceRoot>>,
    mut commands: Commands,
) {
    registry.scan();
    if let Some(sr) = space_root {
        registry.active_space = Some(sr.0.clone());
    }
    info!(
        "UniverseRegistry: found {} universe(s)",
        registry.universes.len()
    );

    // Start the file watcher on the workspace root
    let workspace = workspace_root();
    match UniverseWatcher::new(&workspace) {
        Ok(watcher) => { commands.insert_resource(watcher); }
        Err(e) => { warn!("UniverseWatcher: could not start — {e}"); }
    }
}

fn drain_watcher_events(
    watcher: Option<Res<UniverseWatcher>>,
    mut registry: ResMut<UniverseRegistry>,
) {
    if let Some(w) = watcher {
        if w.poll() {
            registry.rescan_requested = true;
        }
    }
}

fn periodic_scan(
    mut registry: ResMut<UniverseRegistry>,
    space_root: Option<Res<SpaceRoot>>,
) {
    // Sync active_space whenever SpaceRoot changes
    if let Some(sr) = &space_root {
        if sr.is_changed() {
            registry.active_space = Some(sr.0.clone());
        }
    }

    if registry.needs_rescan() {
        registry.scan();
        if let Some(sr) = space_root {
            registry.active_space = Some(sr.0.clone());
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Plugin
// ─────────────────────────────────────────────────────────────────────────────

pub struct UniverseRegistryPlugin;

impl Plugin for UniverseRegistryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UniverseRegistry>()
            .add_systems(Startup, startup_scan)
            .add_systems(Update, (drain_watcher_events, periodic_scan).chain());
    }
}
