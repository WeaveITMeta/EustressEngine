//! `<workspace>/.eustress/instances/<pid>.json` — the per-instance registry
//! record, the multi-instance complement to `engine.port`.
//!
//! `engine.port` is one slot per Universe: the last engine to start owns it
//! and the first to exit deletes it, so two engines open on two Spaces of the
//! SAME Universe cannot both be discovered through it. This record is keyed
//! by PID instead — collision-free, enumerable — so an orchestrator (the
//! `eustress` CLI, an agent) can list every running instance and drive each
//! one by its own port. The record shape is
//! [`eustress_bridge_client::InstanceRecord`], shared with the readers.
//!
//! Lifecycle mirrors [`super::port_file::PortFile`]: written once the bridge
//! is bound, re-written when the loaded Space changes, removed on `Drop`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy::prelude::*;
use eustress_bridge_client::{instance_file_path, InstanceKind, InstanceRecord};

/// Which shell this process is, for the registry record. Inserted by the
/// headless bin; the editor leaves it absent and is reported as `Editor`.
#[derive(Resource, Debug, Clone, Copy)]
pub struct BridgeInstanceKind(pub InstanceKind);

/// Owns the on-disk record so it is removed when the bridge shuts down.
pub struct InstanceFile {
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
        let path = instance_file_path(workspace_root, record.pid);
        let this = Self { path, record };
        this.flush()?;
        Ok(this)
    }

    /// Re-point the record at a newly loaded Space (runtime Space switch or
    /// the first load after boot). Keeps `pid`/`port`/`started_at`.
    pub fn update_space(&mut self, space: &Path, universe: &Path) -> std::io::Result<()> {
        self.record.space = Some(space.to_path_buf());
        self.record.universe = Some(universe.to_path_buf());
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
