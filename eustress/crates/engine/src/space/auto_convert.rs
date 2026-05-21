//! Automatic, in-process TOML→Fjall **import** (dual model).
//!
//! DIRECTION CHANGE 2026-05-17 — binary-first **with TOML import**:
//! the Documents `Eustress/<Universe>/<Space>/` TOML hierarchy and the
//! binary Fjall store COEXIST. This is now an IMPORT-ONLY seed: when a
//! Space's Fjall `tree` partition is empty (first open), bring the disk
//! TOML into Fjall so the binary runtime store reflects it. It NEVER
//! removes, relocates, or "migrates away" the TOML — the hierarchy
//! stays as the editable import source and is honored at runtime
//! (runtime TOML edits flow through the file-watcher; full
//! TOML-edit→Fjall sync is the remaining runtime-honor wire).
//!
//! Binary ECS Fjall remains the PRIMARY runtime store. The import is
//! additive (`import_space` never clears the tree), so direct-to-DB
//! content (e.g. a generated benchmark grid) is preserved.
//!
//! (Previous version moved loose trees to `.eustress/trash/` and
//! stamped `header.migrated_at`. That destructive "no loose files"
//! behaviour is REMOVED — TOML coexists now. With nothing stamping
//! `migrated_at`, `space_ops::space_is_migrated()` stays false, so the
//! disk-suppressing gates added earlier are dormant and the normal
//! TOML-maintenance paths run.)

use std::path::Path;

use eustress_worlddb::WorldDb;

/// Seed the binary store from the Space's TOML hierarchy if needed.
/// Returns `true` when it's safe to source the Space from Fjall, and
/// `false` only when the tree is empty AND the seed import failed (the
/// caller then falls back to the disk/TOML source so the engine still
/// boots and reads TOML directly).
pub fn convert_space_if_needed(space_root: &Path, db: &dyn WorldDb) -> bool {
    match db.tree_is_empty() {
        Ok(true) => {
            // First open of this Space's DB → seed it from the disk
            // TOML hierarchy (additive). TOML is KEPT.
            match eustress_worlddb::import::import_space(db, space_root) {
                Ok(s) => {
                    tracing::info!(
                        target: "eustress_engine::world_db",
                        files = s.files_imported,
                        dirs = s.dirs_walked,
                        bytes = s.bytes_imported,
                        "seed: TOML→Fjall import (dual model — TOML hierarchy kept as import source)"
                    );
                    true
                }
                Err(e) => {
                    tracing::warn!(
                        target: "eustress_engine::world_db",
                        error = %e,
                        "seed import failed — sourcing this Space from disk/TOML"
                    );
                    false
                }
            }
        }
        // Already seeded — Fjall is the runtime store; runtime TOML
        // edits are picked up via the file-watcher.
        Ok(false) => true,
        Err(e) => {
            tracing::warn!(
                target: "eustress_engine::world_db",
                error = %e,
                "tree_is_empty check failed — sourcing this Space from disk/TOML"
            );
            false
        }
    }
}
