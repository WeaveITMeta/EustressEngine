//! # LSP child-process launcher
//!
//! When Studio starts, we spawn `eustress-lsp.exe --tcp --port-file <path>`
//! as a child process so external IDEs (Windsurf, VS Code, Cursor) can
//! connect to a live LSP server without having to spawn their own. The
//! child's bound port is written to `<universe>/.eustress/lsp.port`; the
//! VS Code extension reads that file to pick TCP over stdio.
//!
//! ## Binary resolution
//!
//! The child binary is looked up, in order:
//!
//! 1. `EUSTRESS_LSP_BIN` env var (explicit override for CI / tests).
//! 2. Next to the currently-running engine executable — this is the
//!    production path: both binaries ship together in the installer.
//! 3. `target/release/eustress-lsp(.exe)` relative to the workspace —
//!    the dev-build path.
//! 4. `target/debug/eustress-lsp(.exe)` — last resort for
//!    cargo-run sessions.
//!
//! If none exist, the launcher logs a single warning and does nothing.
//! External IDEs fall back to spawning the binary themselves via stdio,
//! which still works as long as the LSP is on `PATH`.
//!
//! ## Lifecycle
//!
//! * `setup_spawn_lsp_child` runs on `Startup` — spawns the child,
//!   stores the handle + port-file path in [`LspChild`].
//! * `shutdown_lsp_child` runs on `AppExit` — SIGTERM / `Kill`'s the
//!   child and removes the port file.
//! * Missing binary = no child = no error. This is the expected case
//!   on a cargo-run of the bare engine without the `lsp` feature bin
//!   built; we surface it once at info level and move on.

use bevy::app::{App, AppExit, Plugin, Startup};
use bevy::ecs::resource::Resource;
use bevy::log::{info, warn};
use bevy::prelude::*;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

/// Tracks the spawned `eustress-lsp` child process + its sentinel file
/// so the shutdown system can clean both up. Stored as a Bevy resource
/// so ownership is app-scoped — dropping the app drops the child.
///
/// `universe` records which Universe root the current child is serving.
/// When the user switches to a Space in a different Universe via the
/// Universes panel, the launcher system compares the new `SpaceRoot`'s
/// Universe against this field; a mismatch triggers tear-down of the old
/// child and a fresh spawn rooted in the new Universe. Same-Universe
/// Space switches are a no-op — both Spaces share the same port file.
///
/// `binary_missing` short-circuits the spawn-retry loop: once we've
/// confirmed the binary can't be resolved, there's no point re-checking
/// every frame. Flipped to `true` after the first failed lookup.
#[derive(Resource, Default)]
pub struct LspChild {
    child: Option<Child>,
    port_file: Option<PathBuf>,
    universe: Option<PathBuf>,
    binary_missing: bool,
}

impl LspChild {
    /// Tear down the current child + its port file. Called when the
    /// Universe changes or the app exits. Leaves `binary_missing`
    /// untouched so a binary that was missing before stays missing
    /// across a Universe swap.
    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(path) = self.port_file.take() {
            let _ = std::fs::remove_file(&path);
        }
        self.universe = None;
    }
}

impl Drop for LspChild {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct LspLauncherPlugin;

impl Plugin for LspLauncherPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LspChild>()
            // Spawn runs every frame until it succeeds or the binary is
            // confirmed missing. We can't use `Startup` because `SpaceRoot`
            // isn't populated with a real Universe until after the first few
            // frames (scene loader, Space open, etc.), and spawning without
            // a Universe means no port file — external IDEs would have no
            // way to discover the TCP port.
            .add_systems(Update, (
                maybe_spawn_lsp_child,
                shutdown_lsp_child_on_exit,
            ));
    }
}

// ─── Binary resolution ──────────────────────────────────────────────

fn resolve_lsp_binary() -> Option<PathBuf> {
    let exe_suffix = if cfg!(windows) { ".exe" } else { "" };
    let name = format!("eustress-lsp{}", exe_suffix);

    // 1. Env override
    if let Ok(p) = std::env::var("EUSTRESS_LSP_BIN") {
        let pb = PathBuf::from(p);
        if pb.exists() { return Some(pb); }
    }

    // 2. Next to the engine exe (production install layout).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let c = dir.join(&name);
            if c.exists() { return Some(c); }
        }
    }

    // 3. Dev-build paths — `env!("CARGO_MANIFEST_DIR")` bakes the engine
    //    crate path in at compile time; `std::env::var` would be wrong
    //    because the env var only exists during cargo's build process,
    //    not when running the compiled binary. We check release first
    //    (often present even during a debug cargo-watch because another
    //    build already produced it) so external IDEs get the faster
    //    server when available.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for rel in &[
        "../../target/release",
        "../../target/debug",
        "../target/release",
        "../target/debug",
    ] {
        let c = manifest.join(rel).join(&name);
        if c.exists() {
            return Some(c);
        }
    }

    None
}

// ─── Spawn (per-frame, idempotent until success) ────────────────────

fn maybe_spawn_lsp_child(
    mut lsp: ResMut<LspChild>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
) {
    // We've given up on the binary — no point retrying each frame.
    if lsp.binary_missing {
        return;
    }

    // Resolve the current Universe. Preferred source: the live
    // `SpaceRoot` resource (picks up Universe-switch changes). Fallback:
    // the same function `SpaceRoot::default()` itself calls — this
    // covers the startup window where the resource extractor returns
    // `None` for reasons we can't easily fix from here. In practice
    // `default_space_root()` walks the last-opened-space settings + the
    // first alphabetical space on disk, which matches what Studio will
    // land on anyway.
    //
    // `nearest_universe` walks up looking for the closest ancestor
    // containing `Spaces/` — same rule the MCP server and external
    // IDEs use, keeping "what is a Universe" consistent across tooling.
    let space_path = space_root
        .as_deref()
        .map(|sr| sr.0.clone())
        .unwrap_or_else(crate::space::default_space_root);
    let Some(universe) = nearest_universe(&space_path) else {
        write_launcher_diag(&format!(
            "waiting: no Universe resolved (space_path={})",
            space_path.display(),
        ));
        return;
    };

    // Same Universe as the running child? Nothing to do — Space switches
    // within a Universe share one port file.
    if lsp.universe.as_deref() == Some(universe.as_path()) && lsp.child.is_some() {
        return;
    }

    // Universe changed (or first spawn). Tear down any existing child
    // before starting the new one so we don't leak processes and so the
    // old port file doesn't linger as a stale breadcrumb.
    if lsp.child.is_some() {
        info!(
            "LSP launcher: Universe changed ({:?} → {}), restarting child",
            lsp.universe.as_deref().map(|p| p.display().to_string()),
            universe.display(),
        );
        lsp.stop();
    }

    let port_file = universe.join(".eustress").join("lsp.port");

    let Some(binary) = resolve_lsp_binary() else {
        info!("LSP launcher: eustress-lsp binary not found — external IDEs will need to spawn their own via stdio");
        write_launcher_diag(&format!(
            "FAILED: binary not found.\nsearched:\n  \
             EUSTRESS_LSP_BIN={:?}\n  \
             next-to-exe: {:?}\n  \
             CARGO_MANIFEST_DIR: {}",
            std::env::var("EUSTRESS_LSP_BIN").ok(),
            std::env::current_exe().ok().and_then(|p| p.parent().map(|q| q.to_path_buf())),
            env!("CARGO_MANIFEST_DIR"),
        ));
        lsp.binary_missing = true;
        return;
    };

    if let Some(parent) = port_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut cmd = Command::new(&binary);
    cmd.arg("--tcp").arg("--port-file").arg(&port_file);
    // Detach stdio — the launcher doesn't read from the child; external
    // IDEs read the port file. Pipe stderr so LSP warnings surface.
    cmd.stdin(Stdio::null())
       .stdout(Stdio::null())
       .stderr(Stdio::inherit());

    match cmd.spawn() {
        Ok(child) => {
            let pid = child.id();
            info!(
                "LSP launcher: spawned eustress-lsp pid={} ({}), port-file={}",
                pid, binary.display(), port_file.display(),
            );
            write_launcher_diag(&format!(
                "SPAWNED ok\n  pid: {}\n  binary: {}\n  port_file: {}\n  universe: {}",
                pid, binary.display(), port_file.display(), universe.display(),
            ));
            lsp.child = Some(child);
            lsp.port_file = Some(port_file);
            lsp.universe = Some(universe);
        }
        Err(e) => {
            warn!("LSP launcher: failed to spawn {} — {}", binary.display(), e);
            write_launcher_diag(&format!(
                "FAILED: spawn error.\n  binary: {}\n  error: {}",
                binary.display(), e,
            ));
            lsp.binary_missing = true; // Stop retrying — spawn error is persistent.
        }
    }
}

/// Write one-line status to `<temp>/eustress-lsp-launcher.log` so
/// operators can inspect launcher state without needing stdout access to
/// the engine process. Overwrites on each call — only the latest state
/// is relevant. Errors ignored; diagnostics shouldn't take down the app.
fn write_launcher_diag(msg: &str) {
    let path = std::env::temp_dir().join("eustress-lsp-launcher.log");
    let stamped = format!(
        "[{}] {msg}\n",
        chrono::Utc::now().to_rfc3339(),
    );
    let _ = std::fs::write(&path, stamped);
}

// ─── Universe resolution ────────────────────────────────────────────

/// Walk up from `start` looking for the enclosing Universe directory —
/// any ancestor (including `start` itself) that contains a `Spaces/`
/// subdirectory. Bounded to 16 iterations so a pathological symlink
/// cycle can't hang the engine. Returns the Universe root, or `None`
/// if the path isn't inside one.
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

// ─── Shutdown ───────────────────────────────────────────────────────

fn shutdown_lsp_child_on_exit(
    mut exit: MessageReader<AppExit>,
    mut lsp: ResMut<LspChild>,
) {
    if exit.read().next().is_some() {
        // Explicit cleanup on AppExit. `Drop` still runs on teardown as
        // belt-and-braces, but by then AppExit has already propagated
        // and the port file could linger long enough for external IDEs
        // to try a stale TCP port.
        lsp.stop();
        info!("LSP launcher: child process + port file cleaned up");
    }
}
