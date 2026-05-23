//! Reseed a Space subtree's TOMLs from disk into the Fjall `tree`
//! partition, and drop their stale `#bin` binary caches.
//!
//! ## Why this exists
//!
//! For a NON-migrated Space (no `header.migrated_at`) the on-disk TOML
//! hierarchy is the human source of truth, but `world_db_plugin` still
//! installs `FjallSource` whenever the tree is non-empty, so the loader
//! serves the `tree` partition — which was seeded from disk at an EARLIER
//! point and can be stale. The file-watcher only reconciles disk→tree for
//! edits made WHILE the engine runs, so a CLOSED-engine disk edit or a
//! `git checkout` of clobbered files (the V-Cell custom-mesh recovery)
//! does not propagate. This bin does that reconcile out-of-band.
//!
//! It touches ONLY the `tree` partition (the file bytes). The `datastore`
//! partition — game scripts' persistent `DataStoreService` data, e.g. the
//! V-Cell sim state — is NOT touched, so nothing is lost. The Space's
//! engine must be CLOSED (Fjall is single-writer).
//!
//! ## Usage
//! ```
//! # Targeted (recommended for the V-Cell mesh recovery):
//! cargo run -p eustress-engine --bin reseed-space-subtree -- --subdir Workspace/V-Cell
//!
//! # Whole Workspace, or a specific Space:
//! cargo run -p eustress-engine --bin reseed-space-subtree -- --space "C:/path/to/Spaces/Space1" --subdir Workspace
//! ```
//! Defaults: `--space` = Documents/Eustress/Universe1/Spaces/Space1,
//! `--subdir` = `Workspace`.

use std::path::PathBuf;

use eustress_worlddb::WorldDb;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let space = arg(&args, "--space").map(PathBuf::from).unwrap_or_else(default_space);
    let subdir = arg(&args, "--subdir").unwrap_or_else(|| "Workspace".to_string());

    println!("=== Reseed Space subtree (disk → Fjall tree) ===");
    println!("Space:  {}", space.display());
    println!("Subdir: {}", subdir);

    let db_dir = space.join("world.fjalldb");
    if !db_dir.exists() {
        eprintln!(
            "No world.fjalldb at {} — this Space is pure-disk, so the loader already \
             reads the TOMLs directly; nothing to reseed.",
            db_dir.display()
        );
        return;
    }

    let db = match eustress_worlddb::backend::open(&db_dir) {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "Failed to open {} ({e}). Is the engine still running? Close it first \
                 (Fjall is single-writer).",
                db_dir.display()
            );
            std::process::exit(1);
        }
    };

    // Walk the disk subtree; for each `.toml`, overwrite the tree key with
    // the current disk bytes and drop the matching `#bin` cache.
    let scan_root = {
        let mut p = space.clone();
        for seg in subdir.split(['/', '\\']) {
            if !seg.is_empty() {
                p.push(seg);
            }
        }
        p
    };
    if !scan_root.exists() {
        eprintln!("Subdir {} does not exist on disk — nothing to reseed.", scan_root.display());
        return;
    }

    let mut reseeded = 0usize;
    let mut stack = vec![scan_root];
    while let Some(dir) = stack.pop() {
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let Ok(stripped) = path.strip_prefix(&space) else {
                continue;
            };
            let rel = stripped.to_string_lossy().replace('\\', "/");
            match std::fs::read(&path) {
                Ok(bytes) => {
                    if let Err(e) = db.put_file(&rel, &bytes) {
                        eprintln!("put_file {rel}: {e}");
                        continue;
                    }
                    // The lazily-built bincode cache shadows the base key
                    // in `active_db::get_instance`; drop it so the next
                    // read re-derives from this fresh TOML. (Idempotent —
                    // a missing key is a no-op.)
                    let _ = db.delete_file(&format!("{rel}#bin"));
                    reseeded += 1;
                }
                Err(e) => eprintln!("read {}: {e}", path.display()),
            }
        }
    }

    if let Err(e) = db.flush() {
        eprintln!("flush failed: {e}");
    }

    println!(
        "Reseeded {reseeded} TOML(s) disk→tree (+ dropped their #bin caches). \
         datastore partition untouched."
    );
    println!("Done. Open the engine — '{subdir}' now serves the current disk content.");
}

fn arg(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn default_space() -> PathBuf {
    dirs::document_dir()
        .map(|d| {
            d.join("Eustress")
                .join("Universe1")
                .join("Spaces")
                .join("Space1")
        })
        .unwrap_or_else(|| PathBuf::from("Space1"))
}
