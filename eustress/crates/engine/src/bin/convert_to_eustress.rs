//! Bulk TOML→Fjall **import** for every Space (dual model).
//!
//! DIRECTION CHANGE 2026-05-17 — binary-first WITH TOML import: the
//! Documents TOML hierarchy and the binary Fjall store COEXIST. This
//! tool now only *imports* (seeds) the disk TOML of each Space into its
//! `world.fjalldb` — additively, and it **keeps the TOML**. It never
//! relocates, deletes, trashes, or stamps "migrated". (The previous
//! version moved loose trees to `.eustress/trash/` and stamped the
//! header; that destructive "no loose files" behaviour is gone.)
//!
//! Mostly redundant with the engine's per-Space seed on open
//! (`space::auto_convert`); kept for bulk pre-seeding all Universes
//! without opening each in the editor.
//!
//! ## Usage
//! ```text
//! cargo run -p eustress-engine --bin convert-to-eustress -- \
//!     [--eustress-root <dir>] [--space <dir>] [--dry-run]
//! ```
//! Default root: `<Documents>/Eustress`. `--space` scopes to one Space;
//! otherwise every `<root>/<Universe>/Spaces/<Space>` is imported.

#[cfg(feature = "world-db")]
use std::path::PathBuf;

#[cfg(not(feature = "world-db"))]
fn main() {
    eprintln!(
        "convert-to-eustress requires the `world-db` feature (it is in the \
         default `core` tier — build without --no-default-features)."
    );
    std::process::exit(2);
}

#[cfg(feature = "world-db")]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dry_run = args.iter().any(|a| a == "--dry-run");

    let spaces: Vec<PathBuf> = if let Some(one) = parse_string_flag(&args, "--space") {
        vec![PathBuf::from(one)]
    } else {
        let root = parse_string_flag(&args, "--eustress-root")
            .map(PathBuf::from)
            .unwrap_or_else(default_eustress_root);
        discover_spaces(&root)
    };

    println!("=== Eustress TOML→Fjall import (dual model — TOML is KEPT) ===");
    println!("Mode:   {}", if dry_run { "DRY-RUN (no writes)" } else { "IMPORT (additive seed)" });
    println!("Spaces: {}", spaces.len());
    for s in &spaces {
        println!("  - {}", s.display());
    }
    println!();

    if spaces.is_empty() {
        eprintln!(
            "No Spaces found. Pass --space <dir> or --eustress-root <dir> \
             (expected layout: <root>/<Universe>/Spaces/<Space>)."
        );
        std::process::exit(1);
    }

    let mut ok = 0usize;
    let mut failed = 0usize;
    for space in &spaces {
        println!("── {}", space.display());
        if dry_run {
            println!("   would import the TOML tree into world.fjalldb (additive; TOML kept)");
            continue;
        }
        match import_one(space) {
            Ok(n) => {
                ok += 1;
                println!("   ✔ imported {n} files (TOML kept on disk)");
            }
            Err(e) => {
                failed += 1;
                eprintln!("   ✖ {e}");
            }
        }
    }

    println!();
    println!("Summary: {ok} imported, {failed} failed, {} total. TOML hierarchy left intact.", spaces.len());
    if failed > 0 {
        std::process::exit(1);
    }
}

/// Additively import one Space's disk TOML into its `world.fjalldb`.
/// Never clears the tree, never removes/relocates the TOML.
#[cfg(feature = "world-db")]
fn import_one(space_root: &std::path::Path) -> Result<usize, String> {
    if !space_root.is_dir() {
        return Err(format!("{:?} is not a directory", space_root));
    }
    let db_dir = space_root.join("world.fjalldb");
    std::fs::create_dir_all(&db_dir).map_err(|e| format!("create {:?}: {e}", db_dir))?;
    let db = eustress_worlddb::backend::open(&db_dir).map_err(|e| format!("open db: {e}"))?;
    let s = eustress_worlddb::import::import_space(db.as_ref(), space_root)
        .map_err(|e| format!("import_space: {e}"))?;
    Ok(s.files_imported)
}

/// `<root>/<Universe>/Spaces/<Space>` discovery.
#[cfg(feature = "world-db")]
fn discover_spaces(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(universes) = std::fs::read_dir(root) else {
        return out;
    };
    for u in universes.flatten() {
        let upath = u.path();
        if !upath.is_dir() {
            continue;
        }
        let Ok(spaces) = std::fs::read_dir(upath.join("Spaces")) else {
            continue;
        };
        for s in spaces.flatten() {
            let sp = s.path();
            if sp.is_dir() {
                out.push(sp);
            }
        }
    }
    out.sort();
    out
}

#[cfg(feature = "world-db")]
fn parse_string_flag(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

#[cfg(feature = "world-db")]
fn default_eustress_root() -> PathBuf {
    if let Some(docs) = dirs::document_dir() {
        let p = docs.join("Eustress");
        if p.is_dir() {
            return p;
        }
    }
    PathBuf::from("Eustress")
}
