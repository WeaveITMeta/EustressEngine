//! `<workspace>/.eustress/instances/<pid>.json` — the per-instance registry
//! record, the multi-instance complement to `engine.port`.
//!
//! `engine.port` is one slot per Universe: it names the Universe's owner,
//! the last engine to write it, so two engines open on two Spaces of the
//! SAME Universe cannot both be discovered through it. This record is keyed
//! by PID instead — collision-free, enumerable — so an orchestrator (the
//! `eustress` CLI, an agent) can list every running instance and drive each
//! one by its own port. The record shape is
//! [`eustress_bridge_client::InstanceRecord`], shared with the readers.
//!
//! Beside the record sits the instance's private IPC directory,
//! `<workspace>/.eustress/instances/<pid>/` (its sim command queue and
//! runtime snapshot; see `simulation::ipc`). This file owns that
//! directory's lifetime too: cleared when the record is first written (a
//! directory already there belongs to a dead process that had this PID
//! before us) and removed with the record.
//!
//! Lifecycle mirrors [`super::port_file::PortFile`]: written once the bridge
//! is bound, re-written when the loaded Space changes, removed on `Drop`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy::prelude::*;
use eustress_bridge_client::{instance_dir, instance_file_path, InstanceKind, InstanceRecord};

/// Which shell this process is, for the registry record. Inserted by the
/// headless bin; the editor leaves it absent and is reported as `Editor`.
#[derive(Resource, Debug, Clone, Copy)]
pub struct BridgeInstanceKind(pub InstanceKind);

/// Owns the on-disk record so it is removed when the bridge shuts down.
pub struct InstanceFile {
    /// The workspace the record currently lives under.
    workspace: PathBuf,
    path: PathBuf,
    record: InstanceRecord,
}

impl InstanceFile {
    /// Write the record for this process under `workspace_root`. The
    /// workspace root is the parent of all Universes — the same directory
    /// the global `engine.port` lands in, so the two discovery mechanisms
    /// always sit side by side.
    pub fn write(
        workspace_root: &Path,
        port: u16,
        kind: InstanceKind,
        space: Option<&Path>,
        universe: Option<&Path>,
    ) -> std::io::Result<Self> {
        let record = InstanceRecord {
            pid: std::process::id(),
            port,
            kind,
            space: space.map(Path::to_path_buf),
            universe: universe.map(Path::to_path_buf),
            started_at: chrono::Utc::now().to_rfc3339(),
        };
        clear_stale_ipc_dir(workspace_root, record.pid);
        let path = instance_file_path(workspace_root, record.pid);
        let this = Self { workspace: workspace_root.to_path_buf(), path, record };
        this.flush()?;
        Ok(this)
    }

    /// Re-point the record at a newly loaded Space (runtime Space switch or
    /// the first load after boot). Keeps `pid`/`port`/`started_at`.
    ///
    /// If the new Universe lives in a different workspace, the record moves
    /// there, because that is where this instance's IPC directory now is
    /// (`simulation::ipc` derives it from the loaded Space) and where a
    /// client that knows the Universe will look.
    pub fn update_space(&mut self, space: &Path, universe: &Path) -> std::io::Result<()> {
        self.record.space = Some(space.to_path_buf());
        self.record.universe = Some(universe.to_path_buf());
        if let Some(workspace) = workspace_root_for(universe) {
            if workspace != self.workspace {
                let _ = std::fs::remove_file(&self.path);
                let _ = std::fs::remove_dir_all(instance_dir(&self.workspace, self.record.pid));
                self.path = instance_file_path(&workspace, self.record.pid);
                self.workspace = workspace;
            }
        }
        self.flush()
    }

    pub fn record(&self) -> &InstanceRecord {
        &self.record
    }

    pub fn display_path(&self) -> String {
        self.path.display().to_string()
    }

    fn flush(&self) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(&self.record)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&self.path, json)
    }
}

impl Drop for InstanceFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir_all(instance_dir(&self.workspace, self.record.pid));
    }
}

/// Remove an IPC directory left under our PID by a process that died
/// without cleaning up. PIDs are reused, and a leftover queue would be
/// drained as if it were addressed to us, a leftover snapshot read as ours.
/// Runs at `Startup`, before any simulation system touches the directory.
fn clear_stale_ipc_dir(workspace_root: &Path, pid: u32) {
    let dir = instance_dir(workspace_root, pid);
    if dir.is_dir() {
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => info!("EngineBridge: cleared stale IPC directory {} (left by an earlier pid {pid})", dir.display()),
            Err(e) => warn!("EngineBridge: could not clear stale IPC directory {}: {e}", dir.display()),
        }
    }
}

/// The workspace root the registry lives under: the parent of `universe`,
/// exactly as the global `engine.port` is placed. `None` when the Universe
/// has no parent (a filesystem root), in which case no record is written.
pub fn workspace_root_for(universe: &Path) -> Option<PathBuf> {
    universe.parent().map(Path::to_path_buf)
}

/// Shared handle type stored on `EngineBridgeHandle`.
pub type SharedInstanceFile = Arc<std::sync::Mutex<InstanceFile>>;
