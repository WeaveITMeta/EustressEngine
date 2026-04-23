//! # Flat SoulScript → folder migration
//!
//! Older versions of the MCP `execute_rune` / `execute_luau` tools wrote
//! flat files directly into `SoulService/` (e.g. `gravity_dance.rune`).
//! The canonical layout is now **folder-per-script**:
//!
//! ```text
//! SoulService/
//!   gravity_dance/
//!     _instance.toml              class_name = "SoulScript"
//!     gravity_dance.rune          canonical source
//!     gravity_dance.md            canonical summary stub
//! ```
//!
//! The Script-Editor center tab, the Soul panel, and the Explorer
//! icon / class detection all key off the folder layout
//! (`center_tabs::script_source_path_canonical` is the source of
//! truth). Flat files still load via the legacy fallback, but the
//! Explorer renders them as plain file nodes with generic icons and
//! no entity metadata.
//!
//! This module scans `SoulService/` once on space load and promotes
//! any flat `.rune` / `.luau` / `.lua` / `.soul` files it finds into
//! their own folders. Idempotent — re-running after a clean pass
//! touches zero files.
//!
//! ## Safety
//!
//! * Only migrates files **directly** inside `SoulService/`. Anything
//!   already in a subfolder is assumed correct and skipped.
//! * Never overwrites an existing folder / file. If a collision would
//!   occur (e.g. both `SoulService/foo.rune` and `SoulService/foo/`
//!   exist) the flat file is left in place and logged.
//! * Atomic move: create target folder first, write `_instance.toml`
//!   + `.md` stub, then `rename` the flat file into the folder. On any
//!   error the original flat file is left untouched.

use bevy::prelude::*;
use std::path::Path;

/// Scan `<space_root>/SoulService/` once and migrate every flat
/// script file into its own folder per the canonical layout. Returns
/// the number of files migrated so callers can log / report.
///
/// Called from the `Added<SpaceRoot>` system below + directly from
/// the CLI path in [`crate::startup`] so a brand-new engine launch
/// fixes up the Space before the file watcher starts polling (which
/// avoids a spurious despawn → respawn cycle on the migrated files).
pub fn migrate_flat_soul_scripts(space_root: &Path) -> usize {
    let soul_dir = space_root.join("SoulService");
    if !soul_dir.is_dir() { return 0; }

    let entries = match std::fs::read_dir(&soul_dir) {
        Ok(e) => e,
        Err(e) => {
            warn!("soul-script migration: cannot read {:?}: {}", soul_dir, e);
            return 0;
        }
    };

    let mut migrated = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() { continue; }
        let fname = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Only promote source files — leave `_service.toml`,
        // documentation, and anything else alone.
        let (stem, ext) = match split_script_ext(&fname) {
            Some(t) => t,
            None => continue,
        };

        let target_folder = soul_dir.join(&stem);
        if target_folder.exists() {
            // Someone already has a `<stem>/` folder — don't overwrite
            // it. Leaving the flat file in place keeps the user's
            // state intact; they can manually reconcile later.
            warn!(
                "soul-script migration: '{}' has both flat + folder — skipping",
                stem,
            );
            continue;
        }

        if let Err(e) = std::fs::create_dir_all(&target_folder) {
            warn!(
                "soul-script migration: create_dir_all({:?}) failed: {}",
                target_folder, e,
            );
            continue;
        }

        let language = if ext == "luau" || ext == "lua" { "luau" } else { "rune" };
        let instance_toml = format!(
            "[metadata]\nclass_name = \"SoulScript\"\nname = \"{stem}\"\narchivable = true\n\n[properties]\nlanguage = \"{language}\"\n",
        );
        if let Err(e) = std::fs::write(target_folder.join("_instance.toml"), instance_toml) {
            warn!(
                "soul-script migration: write _instance.toml failed for '{}': {}",
                stem, e,
            );
            continue;
        }

        // Create `<stem>.md` empty on purpose — same rationale as
        // `write_soul_script_folder`: the Script-Editor Build
        // pipeline uses the `.md` as a soul-spec, and a pre-filled
        // stub biases the first build toward boilerplate. Leaving
        // zero bytes lets the build populate fresh from the `.rune`
        // source (or the user authors it first). Only create when
        // the file doesn't already exist as a sibling flat summary
        // (rare, but older MindSpace authored summaries alongside
        // the script).
        let summary_path = target_folder.join(format!("{}.md", stem));
        if !summary_path.exists() {
            let _ = std::fs::write(&summary_path, "");
        }

        // Finally move the flat source file into the folder under
        // its canonical name. `std::fs::rename` is the atomic
        // single-filesystem case; `copy + remove_file` covers the
        // cross-volume fallback so tests in temp dirs on another
        // drive still migrate cleanly.
        let target_source = target_folder.join(format!("{stem}.{ext}"));
        match std::fs::rename(&path, &target_source) {
            Ok(_) => {
                info!(
                    "📦 Migrated flat SoulScript '{}' → '{}/'",
                    fname, stem,
                );
                migrated += 1;
            }
            Err(_) => {
                if let Err(e) = std::fs::copy(&path, &target_source) {
                    warn!("soul-script migration: copy '{}' failed: {}", fname, e);
                    continue;
                }
                if let Err(e) = std::fs::remove_file(&path) {
                    warn!("soul-script migration: remove flat '{}' failed: {}", fname, e);
                    continue;
                }
                info!(
                    "📦 Migrated (copy+remove) flat SoulScript '{}' → '{}/'",
                    fname, stem,
                );
                migrated += 1;
            }
        }
    }

    if migrated > 0 {
        info!("📦 SoulScript migration: promoted {} flat file(s) to folders", migrated);
    }
    migrated
}

/// Split `"gravity_dance.rune"` into `("gravity_dance", "rune")` when
/// the file is a recognised script source. Returns `None` for
/// anything else so we don't accidentally promote a README or a
/// `.toml`.
fn split_script_ext(fname: &str) -> Option<(String, String)> {
    // Longest extension wins — `.rune` is the common case.
    for ext in ["rune", "luau", "lua", "soul"] {
        let suffix = format!(".{}", ext);
        if let Some(stem) = fname.strip_suffix(&suffix) {
            if !stem.is_empty() {
                return Some((stem.to_string(), ext.to_string()));
            }
        }
    }
    None
}

/// Bevy plugin that runs the migration any time `SpaceRoot` is first
/// inserted or swapped. Uses `Added<SpaceRoot>` so switching between
/// Spaces at runtime also triggers one scan.
pub struct SoulScriptMigrationPlugin;

impl Plugin for SoulScriptMigrationPlugin {
    fn build(&self, app: &mut App) {
        // Runs early so the file watcher's initial scan picks up the
        // migrated folders as the source of truth rather than racing
        // against a half-migrated state.
        app.add_systems(PreUpdate, run_migration_on_space_change);
    }
}

fn run_migration_on_space_change(
    space_root: Option<Res<crate::space::SpaceRoot>>,
    mut last_migrated_path: Local<Option<std::path::PathBuf>>,
) {
    let Some(space_root) = space_root else { return };
    // Only run when the path actually changes — `is_changed` fires
    // on any resource touch; comparing paths makes this truly
    // one-per-space-load.
    let current = space_root.0.clone();
    if last_migrated_path.as_ref() == Some(&current) { return; }
    migrate_flat_soul_scripts(&current);
    *last_migrated_path = Some(current);
}
