//! End-to-end Roblox import demo.
//!
//! Parses a real `.rbxlx` place file and runs it through the full
//! importer (`parse` → `import_into_space`) into a throwaway temp Space,
//! then prints the `ImportReport` and walks the resulting
//! `_instance.toml` tree so you can SEE what the importer produced.
//!
//! Run with:
//!   cargo run -p eustress-roblox-import --example import_demo
//!
//! Optionally pass a path to your own .rbxl/.rbxlx/.rbxm/.rbxmx:
//!   cargo run -p eustress-roblox-import --example import_demo -- C:/path/to/place.rbxl

use std::path::{Path, PathBuf};

use eustress_roblox_import::{import_into_space, parse, ImportOptions};

fn main() {
    // Resolve the input file: CLI arg, else the bundled demo fixture.
    let input: PathBuf = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("examples/fixtures/demo_place.rbxlx")
        });

    println!("════════════════════════════════════════════════════════════");
    println!(" Roblox Import Demo");
    println!("════════════════════════════════════════════════════════════");
    println!(" Input : {}", input.display());

    // 1. Parse the Roblox file into a RobloxDom.
    let dom = match parse(&input) {
        Ok(d) => d,
        Err(e) => {
            eprintln!(" PARSE FAILED: {e}");
            std::process::exit(1);
        }
    };
    println!(" Format: {:?}", dom.format);

    // 2. Fresh temp Space root to import into.
    let space_root = std::env::temp_dir().join(format!(
        "eustress_rbx_import_demo_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&space_root);
    std::fs::create_dir_all(&space_root).expect("create temp space root");
    println!(" Space : {}", space_root.display());
    println!("────────────────────────────────────────────────────────────");

    // 3. Run the import.
    let report = match import_into_space(&dom, &space_root, ImportOptions::default()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!(" IMPORT FAILED: {e}");
            std::process::exit(1);
        }
    };

    // 4. Print the report.
    println!(" IMPORT REPORT");
    println!("   nodes seen     : {}", report.total_nodes_seen);
    println!("   nodes imported : {}", report.total_nodes_imported);
    println!("   events imported: {}", report.events_imported);
    println!("   elapsed        : {:?}", report.elapsed);
    if !report.class_counts.is_empty() {
        println!("   by class:");
        let mut counts = report.class_counts.clone();
        counts.sort_by(|a, b| a.class.cmp(&b.class));
        for c in &counts {
            println!("     {:<18} × {}", c.class, c.count);
        }
    }
    if !report.unmapped_classes.is_empty() {
        println!("   unmapped classes:");
        for u in &report.unmapped_classes {
            println!("     {:<18} × {} (e.g. {})", u.roblox_class, u.count, u.sample_name);
        }
    }
    if !report.skipped_services.is_empty() {
        println!("   services routed to _imported/:");
        for s in &report.skipped_services {
            println!("     {} — {}", s.service, s.reason);
        }
    }
    if !report.asset_warnings.is_empty() {
        println!("   asset warnings: {} (rbxassetid:// — no CDN access)", report.asset_warnings.len());
    }
    if !report.approximations.is_empty() {
        println!("   approximations:");
        for a in &report.approximations {
            println!("     {} ({}→{}): {}", a.entity_path, a.original_class, a.eustress_class, a.reason);
        }
    }

    // 5. Walk the produced _instance.toml tree.
    println!("────────────────────────────────────────────────────────────");
    println!(" PRODUCED FILE TREE (under the temp Space):");
    walk(&space_root, &space_root, 0);

    // 6. Dump one representative _instance.toml so you can eyeball the
    //    actual mapped properties.
    println!("────────────────────────────────────────────────────────────");
    if let Some(part_toml) = find_first_instance(&space_root, "RedBlock") {
        println!(" SAMPLE — RedBlock/_instance.toml:");
        match std::fs::read_to_string(&part_toml) {
            Ok(s) => {
                for line in s.lines() {
                    println!("   {line}");
                }
            }
            Err(e) => println!("   (could not read: {e})"),
        }
    }
    println!("════════════════════════════════════════════════════════════");
    println!(" Temp Space left at: {}", space_root.display());
    println!(" (delete it when done — it's a throwaway)");
}

/// Recursively print the directory tree, marking _instance.toml files.
fn walk(root: &Path, dir: &Path, depth: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut items: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    items.sort_by_key(|e| e.file_name());
    for entry in items {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let indent = "  ".repeat(depth + 1);
        if path.is_dir() {
            println!("{indent}{}/", name);
            walk(root, &path, depth + 1);
        } else {
            let marker = if name == "_instance.toml" { "  ← entity" } else { "" };
            println!("{indent}{}{}", name, marker);
        }
    }
}

/// Find the first _instance.toml whose parent dir name contains `needle`.
fn find_first_instance(dir: &Path, needle: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().map(|n| n.to_string_lossy().contains(needle)).unwrap_or(false) {
                let candidate = path.join("_instance.toml");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
            if let Some(found) = find_first_instance(&path, needle) {
                return Some(found);
            }
        }
    }
    None
}
