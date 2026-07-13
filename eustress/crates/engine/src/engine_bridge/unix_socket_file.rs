//! `.eustress/engine.sock` — Unix domain socket discovery pointer.
//!
//! Mirrors `port_file.rs`'s `.eustress/engine.port` convention, with one
//! necessary twist: the file at `<universe>/.eustress/engine.sock` does
//! NOT contain the actual bound socket. `AF_UNIX` addresses are capped at
//! `sizeof(sockaddr_un.sun_path)` — around 104 bytes on macOS/BSD, 108 on
//! Linux — and Universe paths comfortably exceed that once nested a few
//! directories deep. The motivating case for this whole transport (an MCP
//! connector's sandboxed per-workspace root, e.g.
//! `~/.claude-science/orgs/<uuid>/workspaces/_mcp-eustress/<Universe>/`)
//! runs well past 100 characters on its own, before `.eustress/engine.sock`
//! is even appended — binding there directly fails immediately with
//! `EINVAL` ("path must be shorter than SUN_LEN"), silently defeating the
//! one deployment this transport exists for.
//!
//! Instead we bind the real socket at a short, deterministic path under
//! the system temp directory (`<tmp>/eustress-bridge-<hash>.sock` — always
//! well within the limit regardless of how deep the Universe lives) and
//! write THAT path as text into `<universe>/.eustress/engine.sock`,
//! exactly like `engine.port` holds a port number rather than being a
//! listening socket itself. Discovery is then: read the pointer file,
//! connect to the path inside it.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Compute the short, deterministic bind path for `universe`. The same
/// Universe always maps to the same path across restarts (a stale
/// leftover socket file from an unclean shutdown is removed by
/// `server::bind_unix_listener` before rebinding); different Universes —
/// even ones running concurrently — get different paths so multiple
/// engines never collide.
pub fn bind_path_for(universe: &Path) -> PathBuf {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    universe.hash(&mut hasher);
    std::env::temp_dir().join(format!("eustress-bridge-{:016x}.sock", hasher.finish()))
}

/// Owns the pointer file(s) and the real socket file so the shutdown path
/// can clean all of them up via `Drop` — same rationale as `PortFile`.
pub struct UnixSocketFile {
    /// The discoverable pointer file at `<universe>/.eustress/engine.sock`.
    pointer_path: PathBuf,
    /// The short path under the system temp dir the socket is actually
    /// bound at (this is what `pointer_path`'s contents resolve to).
    bind_path: PathBuf,
    /// Global fallback pointer at the shared workspace root, mirroring
    /// `PortFile::global_path`. `None` for placeholders / no parent.
    global_pointer_path: Option<PathBuf>,
    placeholder: bool,
}

impl UnixSocketFile {
    /// Write the pointer file(s) recording `bind_path` under `universe`.
    pub fn write_for_universe(universe: &Path, bind_path: &Path) -> std::io::Result<Self> {
        let dir = universe.join(".eustress");
        std::fs::create_dir_all(&dir)?;
        let pointer_path = dir.join("engine.sock");
        std::fs::write(&pointer_path, bind_path.to_string_lossy().as_bytes())?;

        // Also write a GLOBAL copy at the shared workspace root, same
        // rationale as `PortFile`: lets a sibling configured for a
        // DIFFERENT universe still discover the live engine. Best-effort —
        // a global write failure must not fail the authoritative write.
        let global_pointer_path = universe.parent().and_then(|ws| {
            let gdir = ws.join(".eustress");
            std::fs::create_dir_all(&gdir).ok()?;
            let gp = gdir.join("engine.sock");
            std::fs::write(&gp, bind_path.to_string_lossy().as_bytes()).ok()?;
            Some(gp)
        });

        Ok(Self {
            pointer_path,
            bind_path: bind_path.to_path_buf(),
            global_pointer_path,
            placeholder: false,
        })
    }

    /// Placeholder used when the unix listener couldn't be bound at all —
    /// mirrors `PortFile::placeholder`.
    pub fn placeholder() -> Self {
        Self {
            pointer_path: PathBuf::new(),
            bind_path: PathBuf::new(),
            global_pointer_path: None,
            placeholder: true,
        }
    }

    pub fn display_path(&self) -> String {
        if self.placeholder {
            "<none — unix socket bind failed>".to_string()
        } else {
            format!(
                "{} -> {}",
                self.pointer_path.display(),
                self.bind_path.display()
            )
        }
    }

    /// The discoverable pointer file's path. The bridge compares this
    /// against the loaded Space's Universe to decide whether the pointer
    /// needs re-pointing (same pattern as `PortFile::path`).
    pub fn pointer_path(&self) -> &Path {
        &self.pointer_path
    }
}

impl Drop for UnixSocketFile {
    fn drop(&mut self) {
        if self.placeholder {
            return;
        }
        let _ = std::fs::remove_file(&self.pointer_path);
        if let Some(gp) = &self.global_pointer_path {
            let _ = std::fs::remove_file(gp);
        }
        // The real socket file too — `UnixListener::bind` doesn't clean up
        // after itself on drop the way a TCP listener's OS-managed port
        // does; the file is a filesystem object that outlives the fd
        // unless explicitly removed.
        let _ = std::fs::remove_file(&self.bind_path);
    }
}
