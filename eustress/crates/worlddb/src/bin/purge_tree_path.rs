//! Purge one entity from EVERY store of a CLOSED Space's `world.fjalldb`.
//!
//! ## Why this exists
//!
//! The engine's own delete (`active_db::purge_path_all_stores`) is the right
//! way to remove an entity, but it needs the running engine's active DB
//! handle. Sometimes the record to remove is in a Space the engine does NOT
//! have open, or the engine cannot be reached, and the record is not on disk
//! at all — a tree-only ghost that `reconcile_disk_toml_into_tree` can never
//! fix and `prune_orphaned_tree_records` can lose to the `#bin` twin and the
//! `entities_uuid` core (which the engine calls "THE resurrector"). That is
//! exactly the case this bin is for: a `GaussianSplats` inserted into the
//! wrong Space, persisted as `tree` keys + a uuid core + a Morton core, and
//! re-mirrored on every open.
//!
//! It removes, for every tree key matching `--match`:
//! - the `tree` key itself and its `#bin` twin,
//! - the `path_to_uuid` / `uuid_to_path` / `class_index` pointers,
//! - the uuid-primary core in `entities_uuid`,
//! - the Morton-keyed core in `entities`, identified by EXACT byte equality
//!   with the uuid core (both are written by the same `mirror_binary_core`
//!   call), never by guessing an id.
//!
//! Dry-run by default. `--apply` trashes every value first to
//! `<space>/.eustress/trash/db-purge-<unix-seconds>/` so the change is
//! reversible, then deletes, flushes, and re-reads every key to prove the
//! deletion. The Space's engine must be CLOSED for that Space (Fjall has no
//! cross-process lock; prove it by opening the newest journal exclusively).
//!
//! ## Usage
//! ```
//! cargo run -p eustress-worlddb --bin purge_tree_path -- \
//!     --space "C:/Users/me/Documents/Eustress/Voltec/Spaces/VCell" --match Aureole
//! cargo run -p eustress-worlddb --bin purge_tree_path -- \
//!     --space "..." --match Aureole --apply
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use eustress_worlddb::rkyv_values::decode_instance_core;
use eustress_worlddb::EntityId;

const BIN_SUFFIX: &str = "#bin";
const MARKER: &str = "/_instance.toml";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(space) = arg(&args, "--space").map(PathBuf::from) else {
        eprintln!("usage: purge_tree_path --space <SpaceDir> --match <substring> [--apply]");
        std::process::exit(2);
    };
    let Some(needle) = arg(&args, "--match") else {
        eprintln!("usage: purge_tree_path --space <SpaceDir> --match <substring> [--apply]");
        std::process::exit(2);
    };
    let apply = args.iter().any(|a| a == "--apply");

    let db_dir = space.join("world.fjalldb");
    if !db_dir.is_dir() {
        eprintln!("no world.fjalldb under {}", space.display());
        std::process::exit(1);
    }

    println!("=== purge_tree_path ({}) ===", if apply { "APPLY" } else { "dry-run" });
    println!("space: {}", space.display());
    println!("match: {needle:?}");

    let db = match eustress_worlddb::backend::open(&db_dir) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("open {} failed: {e}. Is the engine holding this Space?", db_dir.display());
            std::process::exit(1);
        }
    };

    // ── 1. Tree keys ─────────────────────────────────────────────────────
    let mut tree_keys: Vec<String> = Vec::new();
    match db.iter_tree_keys() {
        Ok(it) => {
            for k in it.flatten() {
                if k.contains(&needle) {
                    tree_keys.push(k);
                }
            }
        }
        Err(e) => {
            eprintln!("iter_tree_keys failed: {e}");
            std::process::exit(1);
        }
    }
    tree_keys.sort();
    println!("\n[tree] {} matching key(s):", tree_keys.len());
    let mut tree_values: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for k in &tree_keys {
        let v = db.get_file(k).ok().flatten().unwrap_or_default();
        println!("  {k}  ({} bytes)", v.len());
        tree_values.insert(k.clone(), v);
    }

    // Folder-form rels: strip the `#bin` twin and the `/_instance.toml`
    // marker so both spellings of the same entity collapse to one rel.
    let mut rels: BTreeSet<String> = BTreeSet::new();
    for k in &tree_keys {
        let base = k.strip_suffix(BIN_SUFFIX).unwrap_or(k);
        let base = base.strip_suffix(MARKER).unwrap_or(base);
        rels.insert(base.to_string());
    }

    // ── 2. UUIDs: from the path index, then from the TOML bytes ──────────
    let mut uuids: BTreeMap<[u8; 16], String> = BTreeMap::new(); // uuid → class_name
    let mut path_index_hits: Vec<(String, [u8; 16])> = Vec::new();
    for rel in &rels {
        for probe in [rel.clone(), format!("{rel}{MARKER}")] {
            if let Ok(Some(u)) = db.path_to_uuid(&probe) {
                path_index_hits.push((probe.clone(), u));
                uuids.entry(u).or_default();
            }
        }
    }
    for (k, v) in &tree_values {
        if !k.ends_with(MARKER) {
            continue;
        }
        let text = String::from_utf8_lossy(v);
        let uuid_hex = toml_str(&text, "uuid");
        let class = toml_str(&text, "class_name").unwrap_or_default();
        if let Some(u) = uuid_hex.as_deref().and_then(hex_to_uuid) {
            let e = uuids.entry(u).or_default();
            if e.is_empty() {
                *e = class;
            }
        }
    }
    println!("\n[path_to_uuid] {} hit(s):", path_index_hits.len());
    for (p, u) in &path_index_hits {
        println!("  {p}  →  {}", hex(u));
    }
    println!("\n[uuids] {}:", uuids.len());
    let mut uuid_cores: BTreeMap<[u8; 16], Vec<u8>> = BTreeMap::new();
    for (u, class) in &uuids {
        let back = db.uuid_to_path(u).ok().flatten();
        let core = db.get_entity_core_by_uuid(u).ok().flatten();
        println!(
            "  {}  class={class:?}  uuid_to_path={back:?}  entities_uuid core={}",
            hex(u),
            core.as_ref().map(|c| format!("{} bytes", c.len())).unwrap_or_else(|| "none".into())
        );
        if let Some(c) = core {
            uuid_cores.insert(*u, c);
        }
    }

    // ── 3. Morton cores: exact byte match against the uuid core ──────────
    let all_cores = match db.iter_instance_cores() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("iter_instance_cores failed: {e}");
            std::process::exit(1);
        }
    };
    let mut morton_hits: Vec<(EntityId, [f32; 3], [u8; 16])> = Vec::new();
    for (eid, bytes) in &all_cores {
        for (u, uc) in &uuid_cores {
            if uc == bytes {
                let t = decode_instance_core(bytes).map(|c| c.t).unwrap_or([0.0; 3]);
                morton_hits.push((*eid, t, *u));
            }
        }
    }
    println!(
        "\n[entities] {} Morton core(s) byte-identical to a matched uuid core (of {} total):",
        morton_hits.len(),
        all_cores.len()
    );
    for (eid, t, u) in &morton_hits {
        println!("  EntityId({})  t={t:?}  uuid={}", eid.0, hex(u));
    }
    // Informational only: every GaussianSplats core, so an unmatched one is
    // visible rather than silently left behind.
    let mut gs_cores = 0usize;
    for (eid, bytes) in &all_cores {
        if let Ok(c) = decode_instance_core(bytes) {
            if c.class_name == "GaussianSplats" {
                gs_cores += 1;
                let matched = morton_hits.iter().any(|(e, _, _)| e == eid);
                println!(
                    "    GaussianSplats core EntityId({}) t={:?} color={:?}{}",
                    eid.0,
                    c.t,
                    c.color,
                    if matched { "  ← will purge" } else { "" }
                );
            }
        }
    }
    println!("    ({gs_cores} GaussianSplats core(s) in this Space)");

    if tree_keys.is_empty() && uuids.is_empty() && morton_hits.is_empty() {
        println!("\nnothing matches {needle:?}; nothing to do.");
        return;
    }
    if !apply {
        println!("\ndry-run: nothing changed. Re-run with --apply to purge the above.");
        return;
    }

    // ── 4. Trash first (reversible) ──────────────────────────────────────
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let trash = space.join(".eustress").join("trash").join(format!("db-purge-{stamp}"));
    if let Err(e) = std::fs::create_dir_all(&trash) {
        eprintln!("cannot create {}: {e}", trash.display());
        std::process::exit(1);
    }
    let mut manifest = String::new();
    for (k, v) in &tree_values {
        let f = trash.join(format!("tree__{}", safe_name(k)));
        let _ = std::fs::write(&f, v);
        manifest.push_str(&format!("tree\t{k}\t{}\n", f.display()));
    }
    for (u, c) in &uuid_cores {
        let f = trash.join(format!("entities_uuid__{}.core", hex(u)));
        let _ = std::fs::write(&f, c);
        manifest.push_str(&format!("entities_uuid\t{}\t{}\n", hex(u), f.display()));
    }
    for (eid, t, u) in &morton_hits {
        manifest.push_str(&format!(
            "entities(morton)\tEntityId({})\tt={t:?}\tuuid={}\n",
            eid.0,
            hex(u)
        ));
    }
    for (p, u) in &path_index_hits {
        manifest.push_str(&format!("path_to_uuid\t{p}\t{}\n", hex(u)));
    }
    let _ = std::fs::write(trash.join("manifest.tsv"), &manifest);
    println!("\n[trash] wrote {} entries under {}", manifest.lines().count(), trash.display());

    // ── 5. Delete from every store ───────────────────────────────────────
    for k in &tree_keys {
        if let Err(e) = db.delete_file(k) {
            eprintln!("delete_file {k}: {e}");
        }
    }
    for rel in &rels {
        for probe in [rel.clone(), format!("{rel}{MARKER}")] {
            let _ = db.delete_path_to_uuid(&probe);
        }
    }
    for (u, class) in &uuids {
        let _ = db.delete_entity_by_uuid(u);
        let _ = db.delete_uuid_to_path(u);
        if !class.is_empty() {
            let _ = db.delete_class_index(class, u);
        }
    }
    for (eid, t, _) in &morton_hits {
        if let Err(e) = db.delete_instance_core(*eid, (t[0], t[1], t[2])) {
            eprintln!("delete_instance_core {}: {e}", eid.0);
        }
    }
    if let Err(e) = db.flush() {
        eprintln!("flush failed: {e}");
        std::process::exit(1);
    }

    // ── 6. Verify by re-reading (delete_* are idempotent, so their Ok
    //      proves nothing; only a None on read does) ──────────────────────
    let mut failed = 0usize;
    for k in &tree_keys {
        let gone = matches!(db.get_file(k), Ok(None));
        println!("  verify tree {k}: {}", if gone { "gone" } else { "STILL PRESENT" });
        failed += usize::from(!gone);
    }
    for (u, _) in &uuids {
        let core_gone = matches!(db.get_entity_core_by_uuid(u), Ok(None));
        let path_gone = matches!(db.uuid_to_path(u), Ok(None));
        println!(
            "  verify uuid {}: core {}, uuid_to_path {}",
            hex(u),
            if core_gone { "gone" } else { "STILL PRESENT" },
            if path_gone { "gone" } else { "STILL PRESENT" }
        );
        failed += usize::from(!core_gone) + usize::from(!path_gone);
    }
    for rel in &rels {
        for probe in [rel.clone(), format!("{rel}{MARKER}")] {
            let gone = matches!(db.path_to_uuid(&probe), Ok(None));
            if !gone {
                println!("  verify path_to_uuid {probe}: STILL PRESENT");
                failed += 1;
            }
        }
    }
    if !morton_hits.is_empty() {
        let after = db.iter_instance_cores().unwrap_or_default();
        for (eid, _, _) in &morton_hits {
            let gone = !after.iter().any(|(e, _)| e == eid);
            println!(
                "  verify Morton core EntityId({}): {}",
                eid.0,
                if gone { "gone" } else { "STILL PRESENT" }
            );
            failed += usize::from(!gone);
        }
    }

    if failed == 0 {
        println!("\nPURGE VERIFIED: every matched record is gone. Reversible copy: {}", trash.display());
    } else {
        eprintln!("\nPURGE INCOMPLETE: {failed} record(s) still readable (see above).");
        std::process::exit(1);
    }
}

fn arg(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

/// First `key = "value"` line in a TOML text. Enough for `uuid` and
/// `class_name` in `[metadata]`; not a TOML parser.
fn toml_str(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let l = l.trim();
        let rest = l.strip_prefix(key)?.trim_start().strip_prefix('=')?.trim();
        let rest = rest.strip_prefix('"')?;
        Some(rest.split('"').next()?.to_string())
    })
}

fn hex_to_uuid(s: &str) -> Option<[u8; 16]> {
    let clean: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if clean.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&clean[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn hex(u: &[u8; 16]) -> String {
    u.iter().map(|b| format!("{b:02x}")).collect()
}

fn safe_name(key: &str) -> String {
    key.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect()
}
