//! `.eustress/engine.port` — port discovery file, mirrors the LSP
//! launcher's `.eustress/lsp.port` convention so sibling processes
//! (MCP server, future plugins) find the bridge the same way IDEs
//! find the LSP.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Owns the written file so the shutdown system can clean it up via
/// `Drop`. We still *want* explicit deletion on `AppExit` for
/// correctness (the `Drop` runs only if the resource is removed or
/// the app is dropped cleanly), but the Drop gives us a second chance
/// if shutdown isn't graceful.
///
/// ## Several engines, one Universe
///
/// The file is a single slot: the last engine to write it is the one a
/// Universe-addressed client reaches, and it is the Universe's OWNER for
/// the other single-slot files (`simulation::ipc`). So every removal is
/// compare-and-delete — a file is removed only while it still names OUR
/// port. Deleting unconditionally meant the first engine to exit wiped the
/// port of the engine that had written after it, leaving the Universe
/// with no reachable owner. When the owner does exit (or crashes), a
/// surviving engine re-claims the slot ([`PortFile::reclaim_if_released`]).
pub struct PortFile {
    path: PathBuf,
    /// Global fallback copy at the shared workspace root (parent of all
    /// universes). Lets a sibling (MCP server) configured for a *different*
    /// universe still discover the live engine. `None` for placeholders / when
    /// the universe has no parent. Cleaned up on `Drop` alongside `path`.
    global_path: Option<PathBuf>,
    /// Cleared by [`PortFile::disarm_global`] when a successor `PortFile`
    /// of this same engine has taken over `global_path`, so dropping this
    /// one doesn't delete the copy the successor just wrote.
    global_armed: AtomicBool,
    /// The port written — what `Drop` compares against before deleting.
    port: u16,
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

        // Also write a GLOBAL copy at the shared workspace root (the parent of
        // all universes) so a sibling (MCP server) configured for a DIFFERENT
        // universe can still discover the live engine. The per-universe file
        // disambiguates when multiple engines run; the global is the
        // single-engine fallback. Best-effort: a global write failure must not
        // fail the authoritative per-universe write.
        let global_path = universe.parent().and_then(|ws| {
            let gdir = ws.join(".eustress");
            std::fs::create_dir_all(&gdir).ok()?;
            let gp = gdir.join("engine.port");
            std::fs::write(&gp, port.to_string()).ok()?;
            Some(gp)
        });

        Ok(Self {
            path,
            global_path,
            global_armed: AtomicBool::new(true),
            port,
            placeholder: false,
        })
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
        Self {
            path: PathBuf::new(),
            global_path: None,
            global_armed: AtomicBool::new(false),
            port: 0,
            placeholder: true,
        }
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

    /// The workspace-level copy's path, if one was written.
    pub fn global_path(&self) -> Option<&Path> {
        self.global_path.as_deref()
    }

    /// Hand `global_path` over to a successor: this `PortFile`'s `Drop`
    /// will leave it alone. Used when re-pointing to another Universe of the
    /// same workspace — the successor rewrites the same global file with the
    /// same port, which compare-and-delete alone cannot tell apart.
    pub fn disarm_global(&self) {
        self.global_armed.store(false, Ordering::Relaxed);
    }

    /// Re-write our port into any of our slots its owner has released:
    /// removed on a clean exit (compare-and-delete removes only the
    /// owner's own port), or still naming a port nothing listens on
    /// because the owner crashed. Returns true if the Universe slot was
    /// re-claimed. A slot that names another live engine is left alone:
    /// that engine owns it.
    pub fn reclaim_if_released(&self) -> bool {
        if self.placeholder {
            return false;
        }
        let reclaimed = slot_released(&self.path, self.port)
            && std::fs::write(&self.path, self.port.to_string()).is_ok();
        if self.global_armed.load(Ordering::Relaxed) {
            if let Some(gp) = &self.global_path {
                if slot_released(gp, self.port) {
                    let _ = std::fs::write(gp, self.port.to_string());
                }
            }
        }
        reclaimed
    }
}

/// True when `path` is free for `ours` to take: missing or unreadable, or
/// naming another port with no listener behind it.
fn slot_released(path: &Path, ours: u16) -> bool {
    match eustress_bridge_client::read_port_file(path) {
        None => true,
        Some(p) if p == ours => false,
        Some(p) => !port_is_listening(p),
    }
}

/// A bare loopback connect: a live bridge's listener accepts it in the
/// kernel backlog even while its engine is busy, so this measures only
/// whether the process is alive. The timeout bounds the probe on Windows,
/// where a refused loopback connect is retried for about two seconds
/// before failing.
fn port_is_listening(port: u16) -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(50)).is_ok()
}

impl Drop for PortFile {
    fn drop(&mut self) {
        if self.placeholder {
            return;
        }
        remove_if_ours(&self.path, self.port);
        if self.global_armed.load(Ordering::Relaxed) {
            if let Some(gp) = &self.global_path {
                remove_if_ours(gp, self.port);
            }
        }
    }
}

/// Delete `path` only while it still holds `port`.
fn remove_if_ours(path: &Path, port: u16) {
    if eustress_bridge_client::read_port_file(path) == Some(port) {
        let _ = std::fs::remove_file(path);
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
        if let Some(universe) = open_space_universe(space) {
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

/// The Universe whose `.eustress/engine.port` an engine with `space` open
/// writes: the Universe of the Space that is actually open.
///
/// A Space inside the user's workspace resolves exactly as it always has
/// ([`crate::space::universe_root_for_path`]). A Space outside it (a test
/// harness, a checkout, a dedicated server's download) has its own Universe:
/// the folder that contains it, by the layout rule the rest of the engine
/// uses ([`crate::workshop::product_meshes::universe_root_of`]). `None` when
/// the open Space has no Universe of its own (the scratch Space, or a Space
/// whose containing folder is the workspace itself or above it); the caller
/// then keeps the slot it already has instead of taking the user's.
pub fn open_space_universe(space: &Path) -> Option<PathBuf> {
    if let Some(universe) = crate::space::universe_root_for_path(space) {
        return Some(universe);
    }
    universe_outside_workspace(
        space,
        &crate::space::workspace_root(),
        crate::space::format_migration::is_scratch_space(space),
    )
}

/// The pure part of [`open_space_universe`] for a Space the workspace scan
/// did not place: the folder containing `space` (its parent, or the parent's
/// parent when that is a `Spaces`/`spaces` tier). Refused when `space` is the
/// scratch Space, has no parent, or when that folder is `workspace` or one of
/// its ancestors, so an engine never writes the workspace-level slot as if it
/// were a Universe.
pub(crate) fn universe_outside_workspace(space: &Path, workspace: &Path, is_scratch: bool) -> Option<PathBuf> {
    if is_scratch {
        return None;
    }
    space.parent()?;
    let universe = crate::workshop::product_meshes::universe_root_of(space);
    if workspace.starts_with(&universe) {
        return None;
    }
    Some(universe)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> PathBuf {
        PathBuf::from("/home/user/Documents/Eustress")
    }

    #[test]
    fn a_space_directly_in_a_harness_universe_writes_to_that_universe() {
        let space = PathBuf::from("/repo/eustress/spaces/harness/RH1_sphere_grid");
        assert_eq!(
            universe_outside_workspace(&space, &workspace(), false),
            Some(PathBuf::from("/repo/eustress/spaces/harness"))
        );
    }

    #[test]
    fn a_space_under_a_spaces_tier_writes_to_the_universe_above_it() {
        let space = PathBuf::from("/elsewhere/Universe2/Spaces/Space1");
        assert_eq!(
            universe_outside_workspace(&space, &workspace(), false),
            Some(PathBuf::from("/elsewhere/Universe2"))
        );
    }

    #[test]
    fn a_space_whose_folder_is_the_workspace_never_takes_the_workspace_slot() {
        let in_root = workspace().join("LooseSpace");
        assert_eq!(universe_outside_workspace(&in_root, &workspace(), false), None);
        assert_eq!(universe_outside_workspace(&workspace(), &workspace(), false), None);
    }

    #[test]
    fn the_scratch_space_and_a_parentless_path_have_no_universe() {
        let space = PathBuf::from("/tmp/Eustress scratch Space");
        assert_eq!(universe_outside_workspace(&space, &workspace(), true), None);
        assert_eq!(universe_outside_workspace(Path::new("/"), &workspace(), false), None);
    }

    #[test]
    fn the_port_file_lands_in_the_open_universes_eustress_folder() {
        let dir = std::env::temp_dir().join(format!("eustress-port-file-test-{}", std::process::id()));
        let universe = dir.join("harness");
        let pf = PortFile::write_for_universe(&universe, 1).unwrap();
        assert_eq!(pf.path(), universe.join(".eustress").join("engine.port").as_path());
        assert_eq!(pf.global_path(), Some(dir.join(".eustress").join("engine.port").as_path()));
        drop(pf);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
