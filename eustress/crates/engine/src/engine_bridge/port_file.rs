//! `.eustress/engine.port` — port discovery file, mirrors the LSP
//! launcher's `.eustress/lsp.port` convention so sibling processes
//! (MCP server, future plugins) find the bridge the same way IDEs
//! find the LSP.

use std::path::{Path, PathBuf};

/// Owns the written file so the shutdown system can clean it up via
/// `Drop`. We still *want* explicit deletion on `AppExit` for
/// correctness (the `Drop` runs only if the resource is removed or
/// the app is dropped cleanly), but the Drop gives us a second chance
/// if shutdown isn't graceful.
pub struct PortFile {
    path: PathBuf,
    /// When true, we never actually wrote anything to disk — the
    /// placeholder variant used when no Universe is loaded yet at
    /// startup. Prevents `Drop` from trying to delete a nonexistent
    /// file and logging a spurious warning.
    placeholder: bool,
}

impl PortFile {
    /// Write `port` to `<universe>/.eustress/engine.port`, picking up
    /// the current Universe from the Eustress default documents
    /// folder. We avoid reaching into Bevy resources here because
    /// startup ordering makes `Res<SpaceRoot>` unreliable — this runs
    /// once, early, and falls back to the default root when needed.
    pub fn write_for_current_universe(port: u16) -> std::io::Result<Self> {
        let universe = default_universe_root();
        let dir = universe.join(".eustress");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("engine.port");
        std::fs::write(&path, port.to_string())?;
        Ok(Self { path, placeholder: false })
    }

    /// Placeholder used when the port file couldn't be written — the
    /// port is still reachable via env var / log output, just not via
    /// the sentinel convention.
    pub fn placeholder() -> Self {
        Self { path: PathBuf::new(), placeholder: true }
    }

    pub fn display_path(&self) -> String {
        if self.placeholder {
            "<none — port file write failed>".to_string()
        } else {
            self.path.display().to_string()
        }
    }
}

impl Drop for PortFile {
    fn drop(&mut self) {
        if self.placeholder {
            return;
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Best-effort lookup of the currently-active Universe root. We try,
/// in order:
///
/// 1. `EUSTRESS_UNIVERSE_ROOT` env var — explicit override for
///    tests / CI / multi-instance setups,
/// 2. `~/Documents/Eustress/Universe1` as the default first-Universe
///    convention the rest of the engine assumes when no Space is
///    loaded.
///
/// If neither resolves we return the current directory so
/// `create_dir_all` at least has somewhere to target.
fn default_universe_root() -> PathBuf {
    if let Ok(env_path) = std::env::var("EUSTRESS_UNIVERSE_ROOT") {
        return PathBuf::from(env_path);
    }

    if let Some(docs) = dirs::document_dir() {
        return docs.join("Eustress").join("Universe1");
    }

    PathBuf::from(".")
}

#[allow(dead_code)]
fn universe_for_space(space_root: &Path) -> PathBuf {
    // Walk up until we find a directory whose parent contains a
    // `.eustress` folder OR whose name looks like `Universe*`.
    // Fallback: the space_root's immediate grandparent (Universe/Spaces/SpaceX).
    let mut cur = space_root;
    while let Some(parent) = cur.parent() {
        if parent.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("Universe")).unwrap_or(false) {
            return parent.to_path_buf();
        }
        cur = parent;
    }
    space_root.to_path_buf()
}
