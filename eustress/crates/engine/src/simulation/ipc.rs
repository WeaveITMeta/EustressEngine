//! Where this engine's simulation IPC files live, and whether it owns the
//! Universe's single-slot legacy files.
//!
//! Several engines can have Spaces of the same Universe open at once (the
//! `eustress open` fan-out), so every simulation file an out-of-process client
//! talks through has to be one of two kinds:
//!
//! * **Per-instance** — `<workspace>/.eustress/instances/<pid>/`. Exactly one
//!   engine reads the command queue and exactly one writes the snapshot.
//!   Addressing a specific engine means addressing its PID.
//! * **Per-Universe, owner only** — the legacy `<universe>/.eustress/`
//!   `sim-commands.jsonl` and `runtime-snapshot.json`. They exist for clients
//!   that predate multi-instance and name only a Universe. Exactly one engine
//!   serves them: the Universe's OWNER, the instance whose bridge port is in
//!   `<universe>/.eustress/engine.port` — the same engine `call_engine` would
//!   reach, so a command written there is no longer delivered to whichever
//!   engine happened to poll first.
//!
//! The paths themselves are defined once, in `eustress-bridge-client`, and
//! shared with the tools that write and read them.

use std::path::{Path, PathBuf};

use bevy::prelude::*;

use crate::engine_bridge::EngineBridgeHandle;

/// This instance's simulation IPC locations, re-resolved whenever the loaded
/// Space changes. Resolving once per Space switch keeps the per-frame queue
/// drain to a single `stat` of a (normally absent) file.
#[derive(Resource, Default, Clone, Debug)]
pub struct SimIpc {
    /// The Universe root of the loaded Space.
    pub universe: Option<PathBuf>,
    /// The workspace root — the Universe's parent, where the instance
    /// registry lives.
    pub workspace: Option<PathBuf>,
    /// The loaded Space's folder name (tags telemetry, names recordings).
    pub space_name: Option<String>,
}

impl SimIpc {
    /// This process's id — the key for every per-instance path.
    pub fn pid() -> u32 {
        std::process::id()
    }

    /// This instance's private command queue.
    pub fn instance_queue(&self) -> Option<PathBuf> {
        self.workspace
            .as_deref()
            .map(|ws| eustress_bridge_client::instance_sim_commands_path(ws, Self::pid()))
    }

    /// Where this instance parks its private queue while draining it.
    pub fn instance_queue_claim(&self) -> Option<PathBuf> {
        self.instance_queue().map(|q| q.with_extension("claimed"))
    }

    /// This instance's private runtime snapshot.
    pub fn instance_snapshot(&self) -> Option<PathBuf> {
        self.workspace
            .as_deref()
            .map(|ws| eustress_bridge_client::instance_snapshot_path(ws, Self::pid()))
    }

    /// The Universe's legacy command queue (served by the owner only).
    pub fn legacy_queue(&self) -> Option<PathBuf> {
        self.universe
            .as_deref()
            .map(eustress_bridge_client::universe_sim_commands_path)
    }

    /// Where this instance parks the legacy queue while draining it. Named
    /// per PID so two engines that briefly both believe they own the
    /// Universe (the ≤1 s hand-over when one exits) can never claim into the
    /// same file.
    pub fn legacy_queue_claim(&self) -> Option<PathBuf> {
        self.legacy_queue()
            .map(|q| q.with_file_name(format!("sim-commands.{}.claimed", Self::pid())))
    }

    /// The Universe's legacy runtime snapshot (written by the owner only).
    pub fn legacy_snapshot(&self) -> Option<PathBuf> {
        self.universe
            .as_deref()
            .map(eustress_bridge_client::universe_snapshot_path)
    }

    /// The Universe's shared, per-line-tagged telemetry log.
    pub fn telemetry(&self) -> Option<PathBuf> {
        self.universe
            .as_deref()
            .map(eustress_bridge_client::universe_telemetry_path)
    }

    /// `<universe>/.eustress/knowledge/recordings/<space>/` — where this
    /// Space's recordings are exported.
    pub fn recordings_dir(&self) -> Option<PathBuf> {
        let universe = self.universe.as_deref()?;
        let space = self.space_name.as_deref().unwrap_or("default");
        Some(
            universe
                .join(".eustress")
                .join("knowledge")
                .join("recordings")
                .join(space),
        )
    }

    /// True while this instance owns the Universe's legacy single-slot files.
    ///
    /// `bridge` is the Engine Bridge handle, if the bridge plugin is in this
    /// build at all:
    /// * absent → no bridge, so nothing else could be addressed either: this
    ///   is a single-instance build and the owner by construction;
    /// * present → owner iff the Universe's `engine.port` names our port. A
    ///   bridge that failed to bind has no port and so is never the owner.
    ///
    /// Reads a five-byte file; callers invoke it only when there is legacy
    /// work to do (a queue file exists, a snapshot is due).
    pub fn owns_universe(&self, bridge: Option<&EngineBridgeHandle>) -> bool {
        let Some(universe) = self.universe.as_deref() else {
            return false;
        };
        let Some(bridge) = bridge else {
            return true;
        };
        let Some(ours) = bridge.port else {
            return false;
        };
        eustress_bridge_client::read_port_file(&universe.join(".eustress").join("engine.port"))
            == Some(ours)
    }
}

/// Re-resolve [`SimIpc`] when the loaded Space changes.
pub fn sync_sim_ipc(space_root: Option<Res<crate::space::SpaceRoot>>, mut ipc: ResMut<SimIpc>) {
    let Some(space_root) = space_root else { return };
    if !space_root.is_changed() {
        return;
    }
    let space = space_root.0.as_path();
    let universe = crate::space::universe_root_for_path(space).or_else(|| walk_up_to_universe(space));
    ipc.workspace = universe.as_deref().and_then(Path::parent).map(Path::to_path_buf);
    ipc.space_name = space.file_name().and_then(|n| n.to_str()).map(str::to_owned);
    ipc.universe = universe;
}

/// Fallback for Spaces outside the configured workspace: the nearest ancestor
/// holding a `Spaces/` (or legacy `spaces/`) tier. This is the walk the
/// simulation plugin used to repeat inline every frame.
fn walk_up_to_universe(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .take(16)
        .find(|p| p.join("Spaces").is_dir() || p.join("spaces").is_dir())
        .map(Path::to_path_buf)
}
