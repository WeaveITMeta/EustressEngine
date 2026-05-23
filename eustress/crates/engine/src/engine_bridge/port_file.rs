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
    /// Write `port` to `<universe>/.eustress/engine.port` for an
    /// explicitly-resolved Universe root. Callers that know the
    /// actually-loaded Space (via `Res<SpaceRoot>`) should resolve the
    /// Universe with [`resolve_universe_root`] and call this so the port
    /// file lands beside the Space the engine really opened.
    pub fn write_for_universe(universe: &Path, port: u16) -> std::io::Result<Self> {
        let dir = universe.join(".eustress");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("engine.port");
        std::fs::write(&path, port.to_string())?;
        Ok(Self { path, placeholder: false })
    }

    /// Back-compat convenience: resolve the Universe ourselves (no
    /// `SpaceRoot` in hand) and write. Prefer [`write_for_universe`] when
    /// the caller can supply the loaded Space's root.
    pub fn write_for_current_universe(port: u16) -> std::io::Result<Self> {
        Self::write_for_universe(&resolve_universe_root(None), port)
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

    /// The on-disk path this port file occupies (empty for placeholders).
    /// The bridge compares it against the loaded Space's Universe to decide
    /// whether the port file needs re-pointing.
    pub fn path(&self) -> &Path {
        &self.path
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

/// Resolve the Universe root whose `.eustress/engine.port` sibling
/// processes (MCP server, plugins) read to find the bridge.
///
/// Order:
///   1. `EUSTRESS_UNIVERSE_ROOT` env override — tests / CI / multi-instance.
///   2. The actually-loaded Space's Universe, when a `SpaceRoot` is supplied
///      (handles `--universe Universe2`, runtime Space switches, non-default
///      Universe names). This is authoritative.
///   3. The `.default_universe` marker the launcher records under the
///      workspace root — used when the bridge starts before the Space has
///      loaded (so `SpaceRoot` isn't in the `World` yet).
///   4. The first *real* Universe under the workspace root (a directory
///      that holds a `Spaces/` tier), skipping hidden config dirs.
///   5. `<workspace>/Universe1`, scaffolding a default if absent.
///
/// CRITICAL — every disk path here flows through
/// [`crate::space::workspace_root`] / `default_documents_root`, which
/// deliberately bypass OneDrive's redirected Documents folder. The old
/// code called `dirs::document_dir()` directly, which on a OneDrive
/// "Known Folder Move" install resolves to `OneDrive\Documents`
/// (`OneDrive\Documentos` on Spanish systems) — a directory neither the
/// engine's Space loader nor the MCP server ever looks in, so the port
/// file was effectively invisible and siblings could never connect.
///
/// Note steps 3-5 are only a best-effort *initial* guess: the bridge's
/// `resync_port_file_to_space` system re-points the file to the loaded
/// Space's real Universe (step 2) the moment `SpaceRoot` appears.
pub fn resolve_universe_root(space_root: Option<&Path>) -> PathBuf {
    if let Ok(env_path) = std::env::var("EUSTRESS_UNIVERSE_ROOT") {
        return PathBuf::from(env_path);
    }

    // (2) Authoritative: the Universe of the Space the engine actually opened.
    if let Some(space) = space_root {
        if let Some(universe) = crate::space::universe_root_for_path(space) {
            return universe;
        }
    }

    // (3-5) Best-effort initial guess when the Space hasn't loaded yet:
    // the recorded `.default_universe`, else first real Universe, else
    // `<workspace>/Universe1` — all OneDrive-avoiding, all skipping the
    // hidden config dirs that a naive scan would pick. The bridge's
    // `resync_port_file_to_space` system re-points the file to the loaded
    // Space's real Universe (step 2) the moment `SpaceRoot` appears.
    crate::space::best_default_universe_root()
}
