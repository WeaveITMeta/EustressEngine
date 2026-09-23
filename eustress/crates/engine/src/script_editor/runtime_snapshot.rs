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
//! - Two snapshot paths, same content:
//!   - `<workspace>/.eustress/instances/<pid>/snapshot.json` — this
//!     instance's own, always written. Tools address one engine among
//!     several with a Space of the same Universe open by reading its PID's
//!     snapshot.
//!   - `<universe>/.eustress/runtime-snapshot.json` — the Universe's
//!     legacy single slot, written only by the Universe's owner (the
//!     instance named in `engine.port`; see `simulation::ipc`). With one
//!     engine per Universe that is simply that engine; with several, the
//!     slot no longer flips between whichever wrote last.
//! - Written by [`RuntimeSnapshotPlugin`] at 4 Hz when the engine is
//!   running (250 ms between writes). Stale mtime = stale snapshot =
//!   LSP shows the cached values until a new write lands.
//! - Read by [`read_snapshot`] which caches per-mtime inside the LSP
//!   so repeated hovers in the same frame don't hammer the filesystem.
//!
//! ## What goes in the snapshot
//!
//! - [`RuntimeSnapshot::play_state`] — editing / playing / paused so
//!   hovers can tag themselves "live" vs. "edit-time".
//! - [`RuntimeSnapshot::sim_values`] — every `SimValuesResource` key
//!   MERGED with the current value of every enabled `WatchPointRegistry`
//!   watchpoint, deduped and stable-sorted. Both stores are exported
//!   because subsystems use whichever suits them: scripts and
//!   `set_sim_value` write `SimValuesResource`, while self-contained
//!   simulations (e.g. the ARC-1 nuclear model) record straight into the
//!   watchpoint registry. `SimValuesResource` wins any key collision.
//!   Used for hover-over-`get_sim_value("X")` to show the current value,
//!   for `list_sim_values`, and for string-literal completion inside
//!   `get_sim_value("|")`.
//! - [`RuntimeSnapshot::generated_at`] — RFC-3339 timestamp. Helps
//!   humans reading the JSON directly and lets the LSP detect obvious
//!   clock-skew scenarios (future-dated snapshots from an older host).
//! - Which engine wrote it ([`RuntimeSnapshot::pid`], `space`,
//!   `owns_universe`), the sim clock, and the run ledger
//!   ([`RuntimeSnapshot::runs`]): the current run, the last few completed
//!   runs with their final values, and acknowledgements for ticketed
//!   commands. A client that queued `run` with a ticket waits for exactly
//!   that run to appear here as completed.
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

    /// Process id of the engine that wrote this snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,

    /// Folder name of the Space that engine has loaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space: Option<String>,

    /// Whether that engine owns the Universe's single-slot files (and so
    /// also serves `<universe>/.eustress/sim-commands.jsonl`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owns_universe: Option<bool>,

    /// Sim clock tick count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick: Option<u64>,

    /// Simulated seconds since the run started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sim_time_s: Option<f64>,

    /// The run ledger: `pending`, `current`, `last`, `completed` (with
    /// final values) and `acks`. Shape defined by
    /// `simulation::command::SimRunLedger::to_json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs: Option<serde_json::Value>,
}

impl RuntimeSnapshot {
    /// Canonical path for the snapshot file given a Universe root.
    pub fn path_in_universe(universe: &Path) -> PathBuf {
        eustress_bridge_client::universe_snapshot_path(universe)
    }

    /// Write to the Universe's legacy slot.
    pub fn write_to_universe(&self, universe: &Path) -> std::io::Result<()> {
        self.write_to_path(&Self::path_in_universe(universe))
    }

    /// Write atomically — we write to `.tmp` first then rename so a
    /// crash mid-write never leaves a truncated JSON file for the LSP
    /// to choke on.
    pub fn write_to_path(&self, final_path: &Path) -> std::io::Result<()> {
        if let Some(parent) = final_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = final_path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, final_path)?;
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
        /// True while a background thread is writing the previous snapshot.
        pub in_flight: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl Default for SnapshotState {
        fn default() -> Self {
            Self {
                last_write: Instant::now() - Duration::from_secs(1),
                interval: Duration::from_millis(250),
                in_flight: Default::default(),
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
        // Watchpoints are a SECOND, independent store. Subsystems that own
        // their own physics (the ARC-1 nuclear sim, for one) call
        // `WatchPointRegistry::record` directly and never touch
        // `SimValuesResource`, so exporting only the latter left every such
        // value invisible to the MCP `get_sim_value` / `list_sim_values`
        // tools and to LSP hovers. The single bridge that exists —
        // `record_and_stream_watchpoints` — copies SimValues INTO
        // watchpoints, which is the wrong direction for export (and it
        // early-returns when SimValues is empty). Merge the registry in
        // here so anything recorded as a watchpoint reaches the snapshot.
        watchpoints: Option<Res<eustress_common::simulation::WatchPointRegistry>>,
        clock: Option<Res<eustress_common::simulation::SimulationClock>>,
        ledger: Option<Res<crate::simulation::command::SimRunLedger>>,
        ipc: Option<Res<crate::simulation::ipc::SimIpc>>,
        bridge: Option<Res<crate::engine_bridge::EngineBridgeHandle>>,
    ) {
        // Throttle — write at most every `interval`. Cheap guard; we
        // still run every frame for the readiness check so we pick up
        // changes as soon as the interval elapses.
        if state.last_write.elapsed() < state.interval {
            return;
        }
        // The previous snapshot is still being written (see below): skip
        // this tick; the next one carries newer values anyway.
        if state.in_flight.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }

        // Need a real Universe to know where to write — resolved once per
        // Space switch by `simulation::ipc::sync_sim_ipc`.
        let Some(ipc) = ipc.as_deref() else { return };
        let Some(universe) = ipc.universe.as_deref() else { return };
        let owns_universe = ipc.owns_universe(bridge.as_deref());

        let snap = RuntimeSnapshot {
            generated_at: chrono::Utc::now().to_rfc3339(),
            play_state: match play_state.as_deref().map(|s| *s.get()) {
                Some(crate::play_mode::PlayModeState::Playing) => PlayState::Playing,
                Some(crate::play_mode::PlayModeState::Paused) => PlayState::Paused,
                _ => PlayState::Editing,
            },
            // `SimValuesResource` first — it is what `set_sim_value` writes,
            // so it stays authoritative for any shared key — then every
            // enabled, finite watchpoint. (Disabled watchpoints are skipped
            // because `WatchPoint::record` early-returns while disabled, so
            // `current` is stale; non-finite values because serde_json
            // renders NaN/±inf as `null`, which readers silently drop.)
            sim_values: crate::simulation::command::merged_sim_values(
                sim_values.as_deref(),
                watchpoints.as_deref(),
            ),
            // ECS schema fields are populated by the extended writer
            // system below; kept empty here to avoid querying the full
            // World on every 250ms tick. A separate 2-second timer
            // refreshes these.
            entity_names: BTreeMap::new(),
            component_types: Vec::new(),
            pid: Some(std::process::id()),
            space: ipc.space_name.clone(),
            owns_universe: Some(owns_universe),
            tick: clock.as_deref().map(|c| c.tick_count),
            sim_time_s: clock.as_deref().map(|c| c.simulation_time_s),
            runs: ledger
                .as_deref()
                .map(crate::simulation::command::snapshot_runs_json),
        };

        // This instance's own snapshot is the one that must land: it is
        // how a tool addresses this engine among several on one Universe.
        // The legacy per-Universe copy is written only by the owner.
        let instance_path = ipc.instance_snapshot();
        let legacy_path = owns_universe.then(|| RuntimeSnapshot::path_in_universe(universe));
        if instance_path.is_none() && legacy_path.is_none() {
            return;
        }

        // Each write is a directory check, a temp file and a rename, twice
        // for the owner: milliseconds of file-system time, four times a
        // second. It runs on its own thread so the frame never waits on the
        // disk.
        use std::sync::atomic::Ordering;
        state.in_flight.store(true, Ordering::Release);
        state.last_write = Instant::now();
        let in_flight = state.in_flight.clone();
        std::thread::spawn(move || {
            if let Some(path) = instance_path {
                if let Err(e) = snap.write_to_path(&path) {
                    debug!("runtime snapshot write to {} failed: {e}", path.display());
                }
            }
            if let Some(path) = legacy_path {
                if let Err(e) = snap.write_to_path(&path) {
                    debug!("runtime snapshot write to {} failed: {e}", path.display());
                }
            }
            in_flight.store(false, Ordering::Release);
        });
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
