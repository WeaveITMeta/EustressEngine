//! Batch importer — brings a folder of `.rbxl` places into Eustress
//! Universes WITH asset fetching (meshes → `.glb`, textures/sounds →
//! `assets/…`) and structured reports.
//!
//! Lives in `eustress-roblox-assets` (not `eustress-roblox-import`)
//! because the network fetchers depend on the importer crate — a bin
//! inside the importer could not use them without a dependency cycle.
//!
//! Reads every place from `C:\Users\miksu\Documents\Roblox Import`, runs
//! it through `parse` → `import_into_space` with a
//! `CachingFetcher(LocalFolder? → Network)` chain (byte + negative cache
//! shared at `C:\Users\miksu\Documents\Eustress\.rbx_cache`), and lands
//! each place as a Space under its domain Universe. Per place it writes
//! `<space>/.eustress/import_report.json` (the full `ImportReport`) and,
//! at the end, an aggregate `import_batch_report.json` under the Eustress
//! root — the coverage-audit input (unmapped classes, asset-failure
//! reasons) for closing importer gaps systematically.
//!
//! Usage:
//!   cargo run -p eustress-roblox-assets --bin rbx_import                  # all 47 (skips existing Spaces)
//!   cargo run -p eustress-roblox-assets --bin rbx_import -- "Mobility"    # one universe
//!   cargo run -p eustress-roblox-assets --bin rbx_import -- --clean       # re-import: trash-move existing Spaces first
//!
//! Env knobs (same as the engine's File→Import path):
//!   EUSTRESS_ROBLOX_ASSET_DIR    local asset mirror tried before the network
//!   EUSTRESS_ROBLOX_NO_NETWORK=1 disable the CDN fetcher entirely
//!   EUSTRESS_ROBLOSECURITY       auth cookie for gated assets (never logged)
//!   EUSTRESS_ROBLOX_RETRY_ERRORS=1  retry ids the negative cache marked dead

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eustress_roblox_assets::{CachingFetcher, ChainFetcher, LocalFolderFetcher, NetworkFetcher};
use eustress_roblox_import::{import_into_space, parse, AssetFetcher, ImportOptions};

/// `(rbxl file stem, Universe, Space name)`. The file stem is the on-disk
/// name (some carry export dates / typos); the Space name is the clean
/// display name.
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
    let mut clean = false;
    let mut filter: Option<String> = None;
    for arg in std::env::args().skip(1) {
        if arg == "--clean" {
            clean = true;
        } else {
            filter = Some(arg);
        }
    }

    let src = PathBuf::from(SRC_DIR);
    let eustress = PathBuf::from(EUSTRESS_DIR);
    let fetcher = build_fetcher(&eustress);

    if let Some(f) = &filter {
        println!("Filter: only Universe == {f:?}");
    }
    println!(
        "Asset fetching: {} · clean re-import: {clean}\n",
        if fetcher.is_some() { "ON (cache: <Eustress>/.rbx_cache)" } else { "OFF" }
    );

    let (mut ok, mut fail, mut skipped) = (0u32, 0u32, 0u32);
    let mut total_nodes = 0u64;
    let mut failures: Vec<(String, String)> = Vec::new();
    let mut place_rows: Vec<serde_json::Value> = Vec::new();
    // Aggregated across every imported place: unmapped Roblox classes and
    // digit-stripped asset-failure reasons — the coverage-audit signal.
    let mut agg_unmapped: BTreeMap<String, u64> = BTreeMap::new();
    let mut agg_reasons: BTreeMap<String, u64> = BTreeMap::new();

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
        if space_root.exists() {
            if clean {
                match trash_move(&eustress.join(universe).join("Spaces"), space, &space_root) {
                    Ok(to) => println!("  (moved old Space to {})", to.display()),
                    Err(e) => {
                        println!("FAIL (trash-move: {e})");
                        fail += 1;
                        failures.push((space.to_string(), format!("trash-move: {e}")));
                        continue;
                    }
                }
                print!("[{universe}] {space:<28} … ");
                let _ = std::io::stdout().flush();
            } else {
                println!("SKIP (exists — pass --clean to re-import)");
                skipped += 1;
                continue;
            }
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

        let opts = ImportOptions {
            asset_fetcher: fetcher.clone(),
            ..Default::default()
        };
        match import_into_space(&dom, &space_root, opts) {
            Ok(r) => {
                write_space_toml(&space_root, space);
                total_nodes += r.total_nodes_imported as u64;

                // Per-place structured report → <space>/.eustress/import_report.json
                let report_json = serde_json::to_value(&r).unwrap_or_default();
                let dot = space_root.join(".eustress");
                let _ = std::fs::create_dir_all(&dot);
                let _ = std::fs::write(
                    dot.join("import_report.json"),
                    serde_json::to_string_pretty(&report_json).unwrap_or_default(),
                );

                // Fetched-asset counts (files actually written this import).
                let meshes = count_files(&space_root.join("assets").join("meshes"));
                let textures = count_files(&space_root.join("assets").join("textures"));
                let sounds = count_files(&space_root.join("assets").join("sounds"));
                let warn = report_json
                    .get("asset_warnings")
                    .and_then(|w| w.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);

                // Aggregate unmapped classes + failure reasons.
                if let Some(unmapped) = report_json.get("unmapped_classes").and_then(|v| v.as_array()) {
                    for u in unmapped {
                        let class = u.get("roblox_class").and_then(|v| v.as_str()).unwrap_or("?");
                        let count = u.get("count").and_then(|v| v.as_u64()).unwrap_or(1);
                        *agg_unmapped.entry(class.to_string()).or_default() += count;
                    }
                }
                if let Some(warns) = report_json.get("asset_warnings").and_then(|v| v.as_array()) {
                    for w in warns {
                        if let Some(reason) = w.get("reason").and_then(|v| v.as_str()) {
                            *agg_reasons.entry(strip_digits(reason)).or_default() += 1;
                        }
                    }
                }

                println!(
                    "OK  ({} nodes, {} glb + {} tex + {} snd fetched, {} asset-warn)",
                    r.total_nodes_imported, meshes, textures, sounds, warn
                );
                place_rows.push(serde_json::json!({
                    "universe": universe, "space": space, "ok": true,
                    "nodes": r.total_nodes_imported, "events": r.events_imported,
                    "meshes_fetched": meshes, "textures_fetched": textures,
                    "sounds_fetched": sounds, "asset_warnings": warn,
                }));
                ok += 1;
            }
            Err(e) => {
                println!("IMPORT FAIL: {e}");
                fail += 1;
                failures.push((space.to_string(), format!("import: {e}")));
                place_rows.push(serde_json::json!({
                    "universe": universe, "space": space, "ok": false,
                    "error": e.to_string(),
                }));
            }
        }
    }

    // Aggregate report → <Eustress>/import_batch_report.json
    let mut reasons: Vec<(&String, &u64)> = agg_reasons.iter().collect();
    reasons.sort_by(|a, b| b.1.cmp(a.1));
    let aggregate = serde_json::json!({
        "filter": filter,
        "clean": clean,
        "imported": ok, "failed": fail, "skipped": skipped,
        "total_nodes": total_nodes,
        "places": place_rows,
        "unmapped_classes": agg_unmapped,
        "top_asset_failure_reasons": reasons
            .iter()
            .take(25)
            .map(|(r, c)| serde_json::json!({"reason": r, "count": c}))
            .collect::<Vec<_>>(),
    });
    let agg_path = eustress.join("import_batch_report.json");
    let _ = std::fs::write(
        &agg_path,
        serde_json::to_string_pretty(&aggregate).unwrap_or_default(),
    );

    println!("\n════════════════════════════════════════════");
    println!(" {ok} imported · {fail} failed · {skipped} skipped · {total_nodes} nodes");
    if !agg_unmapped.is_empty() {
        println!(" unmapped classes (aggregate):");
        let mut um: Vec<(&String, &u64)> = agg_unmapped.iter().collect();
        um.sort_by(|a, b| b.1.cmp(a.1));
        for (class, count) in um.iter().take(15) {
            println!("   {class:<30} × {count}");
        }
    }
    if !failures.is_empty() {
        println!(" failures:");
        for (space, why) in &failures {
            println!("   {space}: {why}");
        }
    }
    println!(" aggregate report: {}", agg_path.display());
    println!("════════════════════════════════════════════");
    if fail > 0 {
        std::process::exit(1);
    }
}

/// The engine's File→Import fetcher chain, shared batch-wide: optional
/// local mirror (`EUSTRESS_ROBLOX_ASSET_DIR`) → network (unless
/// `EUSTRESS_ROBLOX_NO_NETWORK=1`, cookie via `EUSTRESS_ROBLOSECURITY`),
/// wrapped in a byte + negative cache at `<Eustress>/.rbx_cache` so every
/// place in the batch (and future engine re-imports pointed here) reuses
/// fetched bytes.
fn build_fetcher(eustress_root: &Path) -> Option<Arc<dyn AssetFetcher>> {
    let mut chain = ChainFetcher::new();
    if let Ok(dir) = std::env::var("EUSTRESS_ROBLOX_ASSET_DIR") {
        if !dir.trim().is_empty() {
            println!("Local asset mirror: {dir}");
            chain.push(Arc::new(LocalFolderFetcher::new(dir)));
        }
    }
    let network_on = std::env::var("EUSTRESS_ROBLOX_NO_NETWORK")
        .map(|v| v.trim().is_empty() || v == "0" || v.eq_ignore_ascii_case("false"))
        .unwrap_or(true);
    if network_on {
        match std::env::var("EUSTRESS_ROBLOSECURITY") {
            Ok(tok) if !tok.trim().is_empty() => {
                chain.push(Arc::new(NetworkFetcher::with_cookie(tok)));
            }
            _ => {
                chain.push(Arc::new(NetworkFetcher::new()));
            }
        }
    }
    if chain.is_empty() {
        return None;
    }
    let cache_dir = eustress_root.join(".rbx_cache");
    Some(Arc::new(CachingFetcher::new(cache_dir, Arc::new(chain))))
}

/// Reversible clean: move an existing Space directory into
/// `<Universe>/Spaces/.trash/<space>-<unix-secs>` instead of deleting it.
fn trash_move(spaces_dir: &Path, space: &str, space_root: &Path) -> std::io::Result<PathBuf> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let trash = spaces_dir.join(".trash");
    std::fs::create_dir_all(&trash)?;
    let dest = trash.join(format!("{space}-{ts}"));
    std::fs::rename(space_root, &dest)?;
    Ok(dest)
}

/// Count regular files directly inside `dir` (0 when absent).
fn count_files(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter(|e| e.path().is_file()).count())
        .unwrap_or(0)
}

/// Group asset-failure reasons by shape: digit runs collapse to `#` so
/// "rbxassetid://123 …" and "rbxassetid://456 …" aggregate together.
fn strip_digits(reason: &str) -> String {
    let mut out = String::with_capacity(reason.len());
    let mut in_digits = false;
    for c in reason.chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('#');
                in_digits = true;
            }
        } else {
            in_digits = false;
            out.push(c);
        }
    }
    out
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
         created = \"2026-07-09T00:00:00.000000000+00:00\"\n\
         last_modified = \"2026-07-09T00:00:00.000000000+00:00\"\n"
    );
    let _ = std::fs::write(space_root.join("space.toml"), toml);
}
