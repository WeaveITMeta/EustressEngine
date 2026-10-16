//! Batch importer — brings a folder of `.rbxl` places into Eustress
//! Universes WITH asset fetching (meshes → `.glb`, textures/sounds →
//! `assets/…`) and structured reports.
//!
//! Lives in `eustress-roblox-assets` (not `eustress-roblox-import`)
//! because the network fetchers depend on the importer crate — a bin
//! inside the importer could not use them without a dependency cycle.
//!
//! Reads every place in its batch table (`rbx_import_places.toml`, see
//! [`read_batch`]), runs it through `parse` → `import_into_space` with a
//! `CachingFetcher(LocalFolder? → Network)` chain (byte + negative cache
//! shared at `<Eustress>/.rbx_cache`), and lands each place as a Space
//! under its Universe. Per place it writes
//! `<space>/.eustress/import_report.json` (the full `ImportReport`) and,
//! at the end, an aggregate `import_batch_report.json` under the Eustress
//! root — the coverage-audit input (unmapped classes, asset-failure
//! reasons) for closing importer gaps systematically.
//!
//! Usage:
//!   cargo run -p eustress-roblox-assets --bin rbx_import                  # the whole table (skips existing Spaces)
//!   cargo run -p eustress-roblox-assets --bin rbx_import -- "Mobility"    # one universe
//!   cargo run -p eustress-roblox-assets --bin rbx_import -- --clean       # re-import: trash-move existing Spaces first
//!   rbx_import --place "<place file stem>" --space "<Space name>"
//!                                          # one place into a named Space (universe from its row,
//!                                          # or --universe <name>); leaves import_batch_report.json alone
//!
//! Folders:
//!   --places <file>   the batch table (default <Eustress>/rbx_import_places.toml);
//!                     a one-place run with --universe needs none
//!   --eustress <dir>  the Eustress root (default <Documents>/Eustress)
//!   --source <dir>    the .rbxl folder (default the table's source_dir,
//!                     else <Documents>/Roblox Import)
//!
//! Env knobs (same as the engine's File→Import path):
//!   EUSTRESS_ROBLOX_ASSET_DIR    local asset mirror tried before the network
//!   EUSTRESS_ROBLOX_NO_NETWORK=1 disable the CDN fetcher entirely
//!   EUSTRESS_ROBLOX_API_KEY      Open Cloud API key (scope legacy-asset:manage)
//!                                for gated assets; wins over the cookie (never logged)
//!   EUSTRESS_ROBLOSECURITY       auth cookie for gated assets (never logged)
//!   EUSTRESS_ROBLOX_RETRY_ERRORS=1  retry ids the negative cache marked dead

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eustress_roblox_assets::{CachingFetcher, ChainFetcher, LocalFolderFetcher, NetworkFetcher};
use eustress_roblox_import::{import_into_space, parse, AssetFetcher, ImportOptions};

/// The batch: the folder the `.rbxl` places are in, and the Universe and
/// Space each one lands in. Read from a TOML table (`--places <file>`, by
/// default `<Eustress>/rbx_import_places.toml`):
///
/// ```toml
/// source_dir = 'C:\Users\me\Documents\Roblox Import'   # optional
///
/// [[place]]
/// file = "Life Simulator 12-14-2024"   # the .rbxl's stem on disk
/// universe = "Life & Wellness"
/// space = "Life Simulator"             # optional; the stem when absent
/// ```
struct Batch {
    source_dir: Option<PathBuf>,
    /// `(rbxl file stem, Universe, Space name)`.
    places: Vec<(String, String, String)>,
}

fn read_batch(path: &Path) -> Result<Batch, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc: toml::Table = text.parse().map_err(|e| format!("{}: {e}", path.display()))?;
    let source_dir = doc.get("source_dir").and_then(|v| v.as_str()).map(PathBuf::from);
    let mut places = Vec::new();
    let rows = doc.get("place").and_then(|v| v.as_array()).map(Vec::as_slice).unwrap_or_default();
    for (i, row) in rows.iter().enumerate() {
        let field = |key: &str| row.get(key).and_then(|v| v.as_str()).map(str::to_string);
        let (Some(file), Some(universe)) = (field("file"), field("universe")) else {
            return Err(format!("{}: [[place]] {} needs `file` and `universe`", path.display(), i + 1));
        };
        let space = field("space").unwrap_or_else(|| file.clone());
        places.push((file, universe, space));
    }
    Ok(Batch { source_dir, places })
}

/// The user's Documents folder (`%USERPROFILE%\Documents`, `$HOME/Documents`).
fn documents_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Documents")
}

fn main() {
    let mut clean = false;
    let mut filter: Option<String> = None;
    let (mut place, mut space_name, mut universe_name) = (None, None, None);
    let (mut places_file, mut eustress_dir, mut source_dir) = (None, None, None);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--clean" => clean = true,
            "--place" => place = args.next(),
            "--space" => space_name = args.next(),
            "--universe" => universe_name = args.next(),
            "--places" => places_file = args.next().map(PathBuf::from),
            "--eustress" => eustress_dir = args.next().map(PathBuf::from),
            "--source" => source_dir = args.next().map(PathBuf::from),
            _ => filter = Some(arg),
        }
    }
    let eustress = eustress_dir.unwrap_or_else(|| documents_dir().join("Eustress"));
    // The batch table. A one-place run that names its Universe needs none.
    let batch_path = places_file.unwrap_or_else(|| eustress.join("rbx_import_places.toml"));
    let batch = if !batch_path.exists() && place.is_some() && universe_name.is_some() {
        Batch { source_dir: None, places: Vec::new() }
    } else {
        read_batch(&batch_path).unwrap_or_else(|e| {
            eprintln!("{e}\n(the batch table: --places <file>, by default <Eustress>/rbx_import_places.toml)");
            std::process::exit(2);
        })
    };
    let src = source_dir
        .or_else(|| batch.source_dir.clone())
        .unwrap_or_else(|| documents_dir().join("Roblox Import"));

    // One place into a named Space (`--place <stem> --space <name>`), or the
    // whole table.
    let one_place = place.is_some();
    let places: Vec<(String, String, String)> = match place {
        Some(stem) => {
            let Some(space) = space_name else {
                eprintln!("--place needs --space <name>: the Space to import into");
                std::process::exit(2);
            };
            let universe = universe_name
                .or_else(|| batch.places.iter().find(|(s, _, _)| *s == stem).map(|(_, u, _)| u.clone()));
            let Some(universe) = universe else {
                eprintln!("--place {stem:?} is not in the table: give --universe <name>");
                std::process::exit(2);
            };
            vec![(stem, universe, space)]
        }
        None => batch.places,
    };

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

    for (stem, universe, space) in &places {
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

    // Aggregate report → <Eustress>/import_batch_report.json (the whole
    // batch's; a one-place run leaves it alone).
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
    if !one_place {
        let _ = std::fs::write(
            &agg_path,
            serde_json::to_string_pretty(&aggregate).unwrap_or_default(),
        );
    }

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
    if !one_place {
        println!(" aggregate report: {}", agg_path.display());
    }
    println!("════════════════════════════════════════════");
    if fail > 0 {
        std::process::exit(1);
    }
}

/// The engine's File→Import fetcher chain, shared batch-wide: optional
/// local mirror (`EUSTRESS_ROBLOX_ASSET_DIR`) → network (unless
/// `EUSTRESS_ROBLOX_NO_NETWORK=1`; credential from `EUSTRESS_ROBLOX_API_KEY`
/// or `EUSTRESS_ROBLOSECURITY`),
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
        let network = NetworkFetcher::from_env();
        if network.is_authenticated() {
            println!("Roblox credential: {}", network.credential_kind());
        } else {
            // Say this UP FRONT. Roblox gates most asset downloads behind
            // authentication, so without a credential every mesh, texture and
            // sound fails: a past 36-place run logged 62,598 rejections and
            // Vehicle Simulator lost 77,047 asset references the same way.
            // Learning that from the report afterwards is too late.
            println!("  WARNING: no Roblox credential is set. Gated assets will fail with HTTP 401");
            println!("           and meshes, textures and sounds will be MISSING. Set");
            println!("           {} (an Open Cloud API key with legacy-asset:manage)", eustress_roblox_assets::API_KEY_ENV);
            println!("           or {} (a .ROBLOSECURITY cookie).", eustress_roblox_assets::COOKIE_ENV);
        }
        chain.push(Arc::new(network));
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

/// The Space's `space.toml` metadata, so the universe registry lists it in
/// the browser, merged into what the import already wrote there (its
/// `transform_rule` and `format`), which it keeps. The engine overwrites
/// `last_modified` and creates the binary DB on first open.
fn write_space_toml(space_root: &Path, name: &str) {
    let path = space_root.join("space.toml");
    let mut doc: toml::Table = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or_default();
    let table = |doc: &mut toml::Table, key: &str| -> toml::Table {
        match doc.remove(key) {
            Some(toml::Value::Table(t)) => t,
            _ => toml::Table::new(),
        }
    };
    let text = |s: &str| toml::Value::String(s.to_string());
    let mut space = table(&mut doc, "space");
    space.insert("name".into(), text(name));
    space.entry("author").or_insert_with(|| text("Simbuilder"));
    space.entry("version").or_insert_with(|| text("0.1.0"));
    space.insert("created_with".into(), text("Eustress Engine (Roblox batch import)"));
    space.entry("transform_rule").or_insert_with(|| text("parent_pose"));
    let mut metadata = table(&mut doc, "metadata");
    metadata.entry("created").or_insert_with(|| text("2026-07-09T00:00:00.000000000+00:00"));
    metadata.entry("last_modified").or_insert_with(|| text("2026-07-09T00:00:00.000000000+00:00"));
    doc.insert("space".into(), toml::Value::Table(space));
    doc.insert("metadata".into(), toml::Value::Table(metadata));
    if let Ok(body) = toml::to_string_pretty(&doc) {
        let _ = std::fs::write(&path, format!("# EEP Space metadata\n{body}"));
    }
}
