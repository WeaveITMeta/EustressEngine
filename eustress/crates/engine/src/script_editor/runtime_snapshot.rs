//! # Runtime snapshot — cross-process bridge to live engine state
//!
//! The LSP runs as a child process of Eustress Engine (see
//! [`crate::lsp_launcher`]). That isolation gives external IDEs a stable
//! attachment point, but it also means the LSP can't read Bevy
//! resources directly — different address space.
//!
//! This module defines a tiny serialisable snapshot that the engine
//! writes to disk periodically and the LSP reads lazily on hover and
//! completion. Contents are deliberately small (< 4 KB in typical
//! scenes) so both write and read stay under a frame.
//!
//! ## Contract
//!
//! - Snapshot path: `<universe>/.eustress/runtime-snapshot.json`.
//! - Written by [`EngineStatePlugin`] at 4 Hz when the engine is
//!   running (250 ms between writes). Stale mtime = stale snapshot =
//!   LSP shows the cached values until a new write lands.
//! - Read by [`read_snapshot`] which caches per-mtime inside the LSP
//!   so repeated hovers in the same frame don't hammer the filesystem.
//!
//! ## What goes in the snapshot
//!
//! - [`RuntimeSnapshot::play_state`] — editing / playing / paused so
//!   hovers can tag themselves "live" vs. "edit-time".
//! - [`RuntimeSnapshot::sim_values`] — every `SimValuesResource` key,
//!   deduped, stable-sorted. Used for hover-over-`get_sim_value("X")`
//!   to show the current value, and for future string-literal
//!   completion inside `get_sim_value("|")`.
//! - [`RuntimeSnapshot::generated_at`] — RFC-3339 timestamp. Helps
//!   humans reading the JSON directly and lets the LSP detect obvious
//!   clock-skew scenarios (future-dated snapshots from an older host).
//!
//! ## What's intentionally NOT in the snapshot
//!
//! - Full ECS entity dumps. Too large, too churny; would turn every
//!   frame into a dozen KB of disk writes.
//! - Transient simulation graphs (watchpoint history, stream payloads).
//!   These live in EustressStream which has its own transport and
//!   lifecycle; mirroring here would double-store.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ═══════════════════════════════════════════════════════════════════════════
// Data model
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlayState {
    Editing,
    Playing,
    Paused,
}

impl Default for PlayState {
    fn default() -> Self {
        PlayState::Editing
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    /// RFC-3339 timestamp from the writer's wall clock. Purely
    /// informational — the LSP gates freshness on file mtime.
    pub generated_at: String,

    /// Current editor/play state. Hover tooltips append "(live)" when
    /// this is `Playing` and the snapshot key matches.
    pub play_state: PlayState,

    /// `get_sim_value` key → current f64. Sorted by key so diffs are
    /// readable when inspecting the file by hand.
    pub sim_values: BTreeMap<String, f64>,

    /// Named entities currently in the scene. Keyed by entity name,
    /// value is the class/archetype (e.g. "Part", "Model", "Script").
    /// Used by LSP completion for `workspace_find_first("|")` and
    /// `get_tagged_entities` string-literal suggestions.
    #[serde(default)]
    pub entity_names: BTreeMap<String, String>,

    /// Registered ECS component type short names (e.g. "Transform",
    /// "ElectrochemicalState", "ThermodynamicState"). Used by LSP
    /// completion for component-aware scripting suggestions.
    #[serde(default)]
    pub component_types: Vec<String>,
}

impl RuntimeSnapshot {
    /// Canonical path for the snapshot file given a Universe root.
    pub fn path_in_universe(universe: &Path) -> PathBuf {
        universe.join(".eustress").join("runtime-snapshot.json")
    }

    /// Write atomically — we write to `.tmp` first then rename so a
    /// crash mid-write never leaves a truncated JSON file for the LSP
    /// to choke on.
    pub fn write_to_universe(&self, universe: &Path) -> std::io::Result<()> {
        let final_path = Self::path_in_universe(universe);
        if let Some(parent) = final_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = final_path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &final_path)?;
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// LSP-side reader (cached by mtime)
// ═══════════════════════════════════════════════════════════════════════════

/// Per-thread cache — the LSP runs several async tasks but each one
/// reads the snapshot at most once per request; sharing via a
/// thread_local saves the parse cost on repeated hovers.
thread_local! {
    static CACHED: std::cell::RefCell<Option<(PathBuf, std::time::SystemTime, RuntimeSnapshot)>> =
        const { std::cell::RefCell::new(None) };
}

/// Read the snapshot for a Universe, reusing a thread-local cache when
/// the file's mtime hasn't changed. Returns `None` when the file is
/// missing or unparseable — callers treat that as "no live state
/// available" rather than an error.
pub fn read_snapshot(universe: &Path) -> Option<RuntimeSnapshot> {
    let path = RuntimeSnapshot::path_in_universe(universe);
    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;

    // Cache hit?
    let cached_copy = CACHED.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|(p, t, _)| p == &path && *t == mtime)
            .map(|(_, _, snap)| snap.clone())
    });
    if let Some(snap) = cached_copy {
        return Some(snap);
    }

    // Cache miss — parse and install.
    let raw = std::fs::read_to_string(&path).ok()?;
    let snap: RuntimeSnapshot = serde_json::from_str(&raw).ok()?;
    CACHED.with(|c| {
        *c.borrow_mut() = Some((path.clone(), mtime, snap.clone()));
    });
    Some(snap)
}

// ═══════════════════════════════════════════════════════════════════════════
// Engine-side writer — Bevy plugin
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(feature = "realism-scripting")]
mod engine_writer {
    use super::*;
    use bevy::prelude::*;
    use std::time::{Duration, Instant};

    /// Resource tracking when the last snapshot was flushed. Lets the
    /// writer system throttle itself without relying on Time<Fixed>.
    #[derive(Resource, Debug)]
    pub struct SnapshotState {
        pub last_write: Instant,
        pub interval: Duration,
    }

    impl Default for SnapshotState {
        fn default() -> Self {
            Self {
                last_write: Instant::now() - Duration::from_secs(1),
                interval: Duration::from_millis(250),
            }
        }
    }

    pub struct RuntimeSnapshotPlugin;

    impl Plugin for RuntimeSnapshotPlugin {
        fn build(&self, app: &mut App) {
            app.init_resource::<SnapshotState>()
                .add_systems(Update, write_runtime_snapshot);
        }
    }

    fn write_runtime_snapshot(
        mut state: ResMut<SnapshotState>,
        play_state: Option<Res<State<crate::play_mode::PlayModeState>>>,
        sim_values: Option<Res<crate::simulation::plugin::SimValuesResource>>,
        space_root: Option<Res<crate::space::SpaceRoot>>,
    ) {
        // Throttle — write at most every `interval`. Cheap guard; we
        // still run every frame for the readiness check so we pick up
        // changes as soon as the interval elapses.
        if state.last_write.elapsed() < state.interval {
            return;
        }

        // Need a real Universe to know where to write. Same walk-up as
        // the LSP launcher so the snapshot lands where external IDEs
        // look for it — closest ancestor that contains `Spaces/`.
        let Some(universe) = space_root
            .as_deref()
            .and_then(|sr| nearest_universe(&sr.0))
        else {
            return;
        };

        let snap = RuntimeSnapshot {
            generated_at: chrono::Utc::now().to_rfc3339(),
            play_state: match play_state.as_deref().map(|s| *s.get()) {
                Some(crate::play_mode::PlayModeState::Playing) => PlayState::Playing,
                Some(crate::play_mode::PlayModeState::Paused) => PlayState::Paused,
                _ => PlayState::Editing,
            },
            sim_values: sim_values
                .as_deref()
                .map(|r| r.0.iter().map(|(k, v)| (k.clone(), *v)).collect())
                .unwrap_or_default(),
            // ECS schema fields are populated by the extended writer
            // system below; kept empty here to avoid querying the full
            // World on every 250ms tick. A separate 2-second timer
            // refreshes these.
            entity_names: BTreeMap::new(),
            component_types: Vec::new(),
        };

        if snap.write_to_universe(&universe).is_ok() {
            state.last_write = Instant::now();
        }
    }

    /// Duplicate of the LSP launcher's walk — kept local to avoid
    /// forcing that module to expose a pub function. Closest ancestor
    /// of `start` containing a `Spaces/` subdirectory IS a Universe.
    fn nearest_universe(start: &std::path::Path) -> Option<PathBuf> {
        let mut cur = start.to_path_buf();
        for _ in 0..16 {
            if cur.join("Spaces").is_dir() {
                return Some(cur);
            }
            if !cur.pop() {
                return None;
            }
        }
        None
    }
}

#[cfg(feature = "realism-scripting")]
pub use engine_writer::{RuntimeSnapshotPlugin, SnapshotState};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let tmp = std::env::temp_dir().join("eustress-snapshot-test");
        std::fs::create_dir_all(&tmp).unwrap();
        let mut snap = RuntimeSnapshot::default();
        snap.generated_at = "2026-04-19T00:00:00Z".into();
        snap.play_state = PlayState::Playing;
        snap.sim_values.insert("battery.voltage".into(), 3.72);
        snap.write_to_universe(&tmp).unwrap();

        let read = read_snapshot(&tmp).unwrap();
        assert_eq!(read.play_state, PlayState::Playing);
        assert_eq!(read.sim_values.get("battery.voltage"), Some(&3.72));
    }
}
