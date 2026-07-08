//! Batch importer — brings a folder of `.rbxl` places into Eustress Universes.
//!
//! Reads every place from `C:\Users\miksu\Documents\Roblox Import`, runs it
//! through the full importer (`parse` → `import_into_space`), and lands each
//! one as a Space under its domain Universe in `C:\Users\miksu\Documents\
//! Eustress`. A minimal `space.toml` is written per Space so the engine's
//! universe registry lists it in the browser; the engine creates the binary
//! `header.bin` / `world.fjalldb` on first open.
//!
//! Run all 47:
//!   cargo run -p eustress-roblox-import --bin import_batch
//! Stage one universe at a time (exact universe name):
//!   cargo run -p eustress-roblox-import --bin import_batch -- "Mobility"

use std::path::{Path, PathBuf};

use eustress_roblox_import::{import_into_space, parse, ImportOptions};

/// `(rbxl file stem, Universe, Space name)`. The file stem is the on-disk name
/// (some carry export dates / typos); the Space name is the clean display name.
const PLACES: &[(&str, &str, &str)] = &[
    // ── Studio Tooling (15) ──
    ("Application Development", "Studio Tooling", "Application Development"),
    ("Plugin Development", "Studio Tooling", "Plugin Development"),
    ("Studio Logger", "Studio Tooling", "Studio Logger"),
    ("Studio Stamper", "Studio Tooling", "Studio Stamper"),
    ("MindSpace Plugins", "Studio Tooling", "MindSpace Plugins"),
    ("Visual Command Suite", "Studio Tooling", "Visual Command Suite"),
    ("Radial Context", "Studio Tooling", "Radial Context"),
    ("Linker", "Studio Tooling", "Linker"),
    ("Bind UI", "Studio Tooling", "Bind UI"),
    ("View", "Studio Tooling", "View"),
    ("Vocal Instancing", "Studio Tooling", "Vocal Instancing"),
    ("Beams", "Studio Tooling", "Beams"),
    ("Flux Matrix", "Studio Tooling", "Flux Matrix"),
    ("Heuristics & Schemeas", "Studio Tooling", "Heuristics & Schemes"),
    ("Telenodes", "Studio Tooling", "Telenodes"),
    // ── Business & Ops (11) ──
    ("Finance+", "Business & Ops", "Finance+"),
    ("TaxAGI", "Business & Ops", "TaxAGI"),
    ("Weave SaaS", "Business & Ops", "Weave SaaS"),
    ("Podcast Formula", "Business & Ops", "Podcast Formula"),
    ("ServerManagement", "Business & Ops", "ServerManagement"),
    ("Auto Team 01-08-2025", "Business & Ops", "Auto Team"),
    ("AutoTeam", "Business & Ops", "AutoTeam"),
    ("Mountain Ascension", "Business & Ops", "Mountain Ascension"),
    ("Place 36", "Business & Ops", "Place 36"),
    ("Super Station", "Business & Ops", "Super Station"),
    ("MindSpace Superstation", "Business & Ops", "MindSpace Superstation"),
    // ── Life & Wellness (10) ──
    ("Life Simulator 12-14-2024", "Life & Wellness", "Life Simulator"),
    ("Calorie Burner", "Life & Wellness", "Calorie Burner"),
    ("Nutrition", "Life & Wellness", "Nutrition"),
    ("Gym", "Life & Wellness", "Gym"),
    ("Pet Care", "Life & Wellness", "Pet Care"),
    ("Day Care", "Life & Wellness", "Day Care"),
    ("Hospital", "Life & Wellness", "Hospital"),
    ("School", "Life & Wellness", "School"),
    ("Restuarant", "Life & Wellness", "Restuarant"),
    ("Saloon Spa", "Life & Wellness", "Saloon Spa"),
    // ── Digital Twin & Data (5) ──
    ("DigitalTwin", "Digital Twin & Data", "DigitalTwin"),
    ("Data Visualizer", "Digital Twin & Data", "Data Visualizer"),
    ("Seed Simulator", "Digital Twin & Data", "Seed Simulator"),
    ("Cybernetics", "Digital Twin & Data", "Cybernetics"),
    ("Rick Engineering", "Digital Twin & Data", "Rick Engineering"),
    // ── Mobility (3) ──
    ("Vehicles Center", "Mobility", "Vehicles Center"),
    ("BikeChain", "Mobility", "BikeChain"),
    ("RideShare", "Mobility", "RideShare"),
    // ── Social & Civic (3) ──
    ("Dating", "Social & Civic", "Dating"),
    ("Relationship Dynamics", "Social & Civic", "Relationship Dynamics"),
    ("Mikhail J Olson Voting Record", "Social & Civic", "Mikhail J Olson Voting Record"),
];

const SRC_DIR: &str = r"C:\Users\miksu\Documents\Roblox Import";
const EUSTRESS_DIR: &str = r"C:\Users\miksu\Documents\Eustress";

fn main() {
    let filter = std::env::args().nth(1);
    let src = PathBuf::from(SRC_DIR);
    let eustress = PathBuf::from(EUSTRESS_DIR);

    if let Some(f) = &filter {
        println!("Filter: only Universe == {f:?}");
    }
    println!(
        "Importing into {} universe(s) under {}\n",
        PLACES
            .iter()
            .map(|(_, u, _)| *u)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        eustress.display()
    );

    let (mut ok, mut fail, mut skipped) = (0u32, 0u32, 0u32);
    let mut total_nodes = 0u64;
    let mut total_events = 0u64;
    let mut failures: Vec<(String, String)> = Vec::new();

    for (stem, universe, space) in PLACES {
        if let Some(f) = &filter {
            if universe != f {
                continue;
            }
        }
        let rbxl = src.join(format!("{stem}.rbxl"));
        let space_root = eustress.join(universe).join("Spaces").join(space);

        print!("[{universe}] {space:<28} … ");
        use std::io::Write;
        let _ = std::io::stdout().flush();

        if !rbxl.exists() {
            println!("SKIP (no file: {})", rbxl.display());
            skipped += 1;
            continue;
        }

        // Universe marker + space root.
        let _ = std::fs::create_dir_all(eustress.join(universe).join(".eustress"));
        if let Err(e) = std::fs::create_dir_all(&space_root) {
            println!("FAIL (mkdir: {e})");
            fail += 1;
            failures.push((space.to_string(), format!("mkdir: {e}")));
            continue;
        }

        let dom = match parse(&rbxl) {
            Ok(d) => d,
            Err(e) => {
                println!("PARSE FAIL: {e}");
                fail += 1;
                failures.push((space.to_string(), format!("parse: {e}")));
                continue;
            }
        };

        match import_into_space(&dom, &space_root, ImportOptions::default()) {
            Ok(r) => {
                write_space_toml(&space_root, space);
                total_nodes += r.total_nodes_imported as u64;
                total_events += r.events_imported as u64;
                let warn = if r.asset_warnings.is_empty() {
                    String::new()
                } else {
                    format!(", {} asset-warn", r.asset_warnings.len())
                };
                println!(
                    "OK  ({} nodes, {} events{warn})",
                    r.total_nodes_imported, r.events_imported
                );
                ok += 1;
            }
            Err(e) => {
                println!("IMPORT FAIL: {e}");
                fail += 1;
                failures.push((space.to_string(), format!("import: {e}")));
            }
        }
    }

    println!("\n════════════════════════════════════════════");
    println!(" {ok} imported · {fail} failed · {skipped} skipped");
    println!(" {total_nodes} nodes, {total_events} events total");
    if !failures.is_empty() {
        println!(" failures:");
        for (space, why) in &failures {
            println!("   {space}: {why}");
        }
    }
    println!("════════════════════════════════════════════");
    if fail > 0 {
        std::process::exit(1);
    }
}

/// Minimal `space.toml` so the universe registry lists the Space in the
/// browser. Mirrors the engine's own format; the engine overwrites
/// `last_modified` and creates the binary DB on first open.
fn write_space_toml(space_root: &Path, name: &str) {
    let toml = format!(
        "# EEP Space metadata\n\
         [space]\n\
         name = \"{name}\"\n\
         author = \"Simbuilder\"\n\
         version = \"0.1.0\"\n\
         created_with = \"Eustress Engine (Roblox batch import)\"\n\
         \n\
         [metadata]\n\
         created = \"2026-07-05T00:00:00.000000000+00:00\"\n\
         last_modified = \"2026-07-05T00:00:00.000000000+00:00\"\n"
    );
    let _ = std::fs::write(space_root.join("space.toml"), toml);
}
