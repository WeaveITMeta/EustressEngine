//! Faithful whole-Space-tree → Fjall importer.
//!
//! One-shot migration that mirrors the ENTIRE Space directory into the
//! Fjall `tree` partition, keyed by Space-relative forward-slash path.
//! Unlike the earlier flat scaffold (Workspace `_instance.toml` only),
//! this captures **everything** so a Fjall-sourced cold load
//! reconstructs the byte-identical scene:
//!
//! - every service (`Workspace/`, `Lighting/`, `MaterialService/`,
//!   `SoulService/`, …) including each `_service.toml`
//! - the full parent/child folder hierarchy (path = hierarchy)
//! - every entity `_instance.toml` at any nesting depth
//! - non-instance files inside entity folders: scripts (`.rune`,
//!   `.luau`), typed TOMLs (`.textlabel.toml`), `.md` docs, meshes
//!
//! ## Exclusions
//!
//! `.eustress/`, `world.fjalldb/`, `.git/`, and any dotfile dir are
//! skipped — they're engine metadata / the DB itself / VCS, not Space
//! content. A symlink loop guard caps recursion depth.
//!
//! ## Idempotency
//!
//! Re-running overwrites identical bytes. Callers gate on
//! [`crate::WorldDb::tree_is_empty`] so a migrated world is never
//! re-seeded from (now-stale) disk.

use std::path::Path;

use crate::backend::WorldDb;
use crate::error::{Error, Result};

/// Directory names skipped at any depth — engine metadata, the DB
/// itself, VCS. Anything else (including unknown service folders) is
/// imported so the migration never silently drops content.
const SKIP_DIRS: &[&str] = &[".eustress", "world.fjalldb", ".git"];

/// Max recursion depth — guards against symlink loops in a Space.
const MAX_DEPTH: usize = 32;

/// Summary returned by [`import_space`].
#[derive(Debug, Clone, Default)]
pub struct ImportSummary {
    /// Files mirrored into the tree partition.
    pub files_imported: usize,
    /// Directories walked.
    pub dirs_walked: usize,
    /// Total bytes written.
    pub bytes_imported: u64,
    /// Files skipped due to read errors (logged at warn).
    pub failures: usize,
}

/// Mirror the entire Space tree rooted at `space_root` into `db`'s
/// tree partition. `space_root` is the on-disk
/// `<universe>/Spaces/<space>/` directory. Returns the
/// [`ImportSummary`] for the engine to surface via the Output panel /
/// a `world.import` telemetry event.
///
/// Keys are forward-slash relative paths from `space_root`
/// (`Lighting/_service.toml`, `Workspace/Tower/MegaTower_Core/_instance.toml`,
/// `SoulService/futuristic_city_generator/futuristic_city_generator.luau`).
pub fn import_space(db: &dyn WorldDb, space_root: &Path) -> Result<ImportSummary> {
    let _span =
        tracing::info_span!("worlddb.import_space", space = %space_root.display()).entered();

    if !space_root.is_dir() {
        return Err(Error::Other(format!(
            "import_space: {:?} is not a directory",
            space_root
        )));
    }

    let mut summary = ImportSummary::default();
    // (absolute_dir, rel_prefix, depth)
    let mut stack: Vec<(std::path::PathBuf, String, usize)> =
        vec![(space_root.to_path_buf(), String::new(), 0)];

    while let Some((dir, rel_prefix, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            tracing::warn!(
                target: "eustress_worlddb::import",
                dir = %dir.display(),
                "skip — exceeded MAX_DEPTH (symlink loop?)"
            );
            summary.failures += 1;
            continue;
        }
        summary.dirs_walked += 1;

        let entries = match std::fs::read_dir(&dir) {
            Ok(it) => it,
            Err(e) => {
                tracing::warn!(
                    target: "eustress_worlddb::import",
                    dir = %dir.display(),
                    error = %e,
                    "skip — read_dir failed"
                );
                summary.failures += 1;
                continue;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };

            if path.is_dir() {
                if SKIP_DIRS.contains(&name) || name.starts_with('.') {
                    continue;
                }
                let child_rel = if rel_prefix.is_empty() {
                    name.to_string()
                } else {
                    format!("{rel_prefix}/{name}")
                };
                stack.push((path, child_rel, depth + 1));
            } else {
                let rel = if rel_prefix.is_empty() {
                    name.to_string()
                } else {
                    format!("{rel_prefix}/{name}")
                };
                match std::fs::read(&path) {
                    Ok(bytes) => {
                        db.put_file(&rel, &bytes)?;
                        summary.files_imported += 1;
                        summary.bytes_imported += bytes.len() as u64;
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "eustress_worlddb::import",
                            file = %path.display(),
                            error = %e,
                            "skip — read failed"
                        );
                        summary.failures += 1;
                    }
                }
            }
        }
    }

    db.flush()?;

    tracing::info!(
        target: "eustress_worlddb::import",
        files = summary.files_imported,
        dirs = summary.dirs_walked,
        bytes = summary.bytes_imported,
        failures = summary.failures,
        "faithful Space-tree import complete"
    );

    Ok(summary)
}
