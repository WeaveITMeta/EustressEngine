//! # Harness scene generator (G1.03), plus three harness drivers (G1.05, G1.08, G1.04)
//!
//! Emits the six capture-harness Spaces of `docs/PROMPTS/02_CAPTURE_HARNESS.md`
//! section 3 from a seed. The generator source is the source of truth; every
//! Space directory under `eustress/spaces/harness/` is derived output and is
//! never edited by hand.
//!
//! | Scene id | Space directory     | Purpose                                   |
//! |----------|---------------------|-------------------------------------------|
//! | S1       | `RH1_sphere_grid`   | material and lighting calibration         |
//! | S2       | `RH2_interior`      | enclosed room, window, two lamps          |
//! | S3       | `RH3_exterior`      | open ground, one declared directional sun |
//! | S4       | `S4_studio_ui`      | studio at a pinned selection and layout   |
//! | S5       | `S5_physics_stress` | mass ratio, stack, chain, impact, rest    |
//! | S6       | `S6_domain_sim`     | copper rod heat conduction with recorder  |
//!
//! ## Usage
//! ```text
//! harness-scenes --seed 42 --out spaces/harness --all [--replace]
//! harness-scenes --seed 42 --out spaces/harness --all --verify-freeze --record <json>
//! harness-scenes capture-selftest --universe <dir> --pose-position X Y Z
//!     --pose-look-at X Y Z --sizes WxH,WxH,... --out <json>
//! harness-scenes frametime --space <dir> --path <camera path> --width W
//!     --height H --csv <path> --summary <path> [--timeout-s N] [--in-place]
//! harness-scenes paths --emit [--paths-dir <dir>] [--seed N]
//! harness-scenes paths --verify --out <json> --space <dir> --paths CP-A,CP-B,CP-S
//!     [--paths-dir <dir>] [--freeze <json>] [--timeout-s N] [--in-place] [--engine <exe>]
//! ```
//!
//! ## Determinism
//!
//! * No wall clock, no process id and no machine path reaches a scene file.
//!   Timestamps inside a scene are the fixed `GENERATOR_EPOCH`.
//! * Every map that reaches output is a `BTreeMap`; files are written and
//!   hashed in byte order of their Space-relative path.
//! * Floats are rounded to 6 decimals before formatting.
//! * Every instance file (`_instance.toml`, `*.instance.toml`) is written in
//!   the exact canonical form the engine's loader converges on
//!   (`eustress_common::class_schema::heal_instance_from_str` followed by
//!   `toml::to_string_pretty`), so opening the Space does not self-heal any
//!   file: the hashed tree is the tree the engine loads. The generator checks
//!   that the canonical form is a fixed point and fails loudly if it is not.
//! * Every `.eustress` manifest and `.eustress/.last_name`, which the engine
//!   writes on open when they are missing (`space_ops.rs` `ensure_manifest_set`,
//!   `migrate_recordings_on_rename`), is emitted with fixed content, and every
//!   service folder the integrity repair would create is emitted, so the open
//!   path writes no scene file it does not already find byte-identical.
//!
//! What the engine still adds when a Space is opened in place is runtime
//! state it owns (`world.fjalldb/`, logs, recordings). A frozen scene must
//! therefore be opened from a copy; `frametime` copies the Space before it
//! launches the engine. `--verify-freeze` reports any such addition in the
//! frozen tree as a difference, which is the honest result.
//!
//! ## Tree hash (the `sha256` in the freeze record)
//!
//! SHA-256 over, for every regular file under the Space directory in byte
//! order of its `/`-separated relative path: `b"F\0" + path + b"\0" +
//! u64-LE byte length + SHA-256(file bytes)`. Directories carry no bytes and
//! are implied by the files in them; the generator emits no empty directory.
//!
//! Numbers in scene content are CONFIG DEFAULT values of this generator.
//! Nothing here is MEASURED.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

/// The source of this binary, embedded so the freeze record names the exact
/// generator that emitted the scenes.
const GENERATOR_SOURCE: &[u8] = include_bytes!("harness_scenes.rs");
const GENERATOR_REL_PATH: &str = "eustress/crates/engine/src/bin/harness_scenes.rs";
const BIN_NAME: &str = "harness-scenes";
/// CONFIG DEFAULT: the fixed timestamp written wherever a scene file carries one.
const GENERATOR_EPOCH: &str = "2026-11-16T00:00:00Z";
/// CONFIG DEFAULT: seed when `--seed` is not given.
const DEFAULT_SEED: u64 = 42;

// ─────────────────────────────────────────────────────────────────────────────
// Scene table
// ─────────────────────────────────────────────────────────────────────────────

struct SceneSpec {
    id: &'static str,
    dir: &'static str,
    title: &'static str,
    build: fn(&mut Scene),
}

const SCENES: &[SceneSpec] = &[
    SceneSpec {
        id: "S1",
        dir: "RH1_sphere_grid",
        title: "Material and lighting: 8x5 roughness x metallic sphere grid, three fixed lights, 18% grey ground, calibration chart",
        build: build_s1,
    },
    SceneSpec {
        id: "S2",
        dir: "RH2_interior",
        title: "Material and coherence: 6 m x 8 m x 3 m room, one window, two lamps of differing colour temperature",
        build: build_s2,
    },
    SceneSpec {
        id: "S3",
        dir: "RH3_exterior",
        title: "Material, motion and coherence: open ground, one declared directional source, occluders 0.2 m to 20 m",
        build: build_s3,
    },
    SceneSpec {
        id: "S4",
        dir: "S4_studio_ui",
        title: "Studio UI: pinned Space, pinned selection, pinned panel layout, pinned window size",
        build: build_s4,
    },
    SceneSpec {
        id: "S5",
        dir: "S5_physics_stress",
        title: "Motion and simulation: 1000:1 mass ratios, settling stack, joint chain, high-restitution impact, 600-tick resting body",
        build: build_s5,
    },
    SceneSpec {
        id: "S6",
        dir: "S6_domain_sim",
        title: "Simulation: copper rod heat conduction (explicit FTCS) with recorder and independent step counter",
        build: build_s6,
    },
];

// ─────────────────────────────────────────────────────────────────────────────
// Entry
// ─────────────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = match args.get(1).map(String::as_str) {
        Some("capture-selftest") => capture_selftest(&args[2..]),
        Some("frametime") => frametime(&args[2..]),
        Some("paths") => paths(&args[2..]),
        Some("tick-selftest") => tick_selftest(&args[2..]),
        _ => generate(&args[1..]),
    };
    std::process::exit(code);
}

// ─────────────────────────────────────────────────────────────────────────────
// Argument helpers (no external dependency)
// ─────────────────────────────────────────────────────────────────────────────

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn values(args: &[String], name: &str, n: usize) -> Option<Vec<String>> {
    let i = args.iter().position(|a| a == name)?;
    let out: Vec<String> = args.iter().skip(i + 1).take(n).cloned().collect();
    (out.len() == n).then_some(out)
}

fn absolute(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().map(|d| d.join(p)).unwrap_or_else(|_| p.to_path_buf())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Generator command
// ─────────────────────────────────────────────────────────────────────────────

fn generate(args: &[String]) -> i32 {
    let seed = match value(args, "--seed") {
        Some(s) => match s.parse::<u64>() {
            Ok(v) => v,
            Err(_) => {
                eprintln!("ERROR: --seed must be an unsigned integer, got {s:?}");
                return 2;
            }
        },
        None => DEFAULT_SEED,
    };
    let Some(out) = value(args, "--out").map(PathBuf::from) else {
        eprintln!("ERROR: --out <dir> is required (the folder that holds the harness Spaces)");
        return 2;
    };
    let selected: Vec<&SceneSpec> = if flag(args, "--all") {
        SCENES.iter().collect()
    } else if let Some(list) = value(args, "--scene") {
        let mut v = Vec::new();
        for id in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match SCENES.iter().find(|s| s.id == id || s.dir == id) {
                Some(s) => v.push(s),
                None => {
                    eprintln!("ERROR: unknown scene {id:?}");
                    return 2;
                }
            }
        }
        v
    } else {
        eprintln!("ERROR: pass --all or --scene S1,S2,...");
        return 2;
    };
    let verify = flag(args, "--verify-freeze");
    let replace = flag(args, "--replace");
    let record = value(args, "--record").map(PathBuf::from);
    if verify && record.is_none() {
        eprintln!("ERROR: --verify-freeze needs --record <json>");
        return 2;
    }

    // Emit every selected scene in memory first; nothing touches disk until
    // all six built and canonicalised cleanly.
    let mut emitted: Vec<(&SceneSpec, BTreeMap<String, Vec<u8>>)> = Vec::new();
    for spec in selected.iter().copied() {
        match emit_scene(spec, seed) {
            Ok(files) => emitted.push((spec, files)),
            Err(e) => {
                eprintln!("ERROR: scene {} ({}) could not be emitted: {e}", spec.id, spec.dir);
                return 1;
            }
        }
    }

    if !verify {
        for (spec, files) in &emitted {
            let dir = out.join(spec.dir);
            if let Err(e) = write_scene_dir(&dir, files, replace) {
                eprintln!("ERROR: {} ({}): {e}", spec.id, spec.dir);
                return 3;
            }
            match tree_digest(&dir) {
                Ok(d) => println!("{} {:<20} sha256:{}  written ({} files, {} bytes)", spec.id, spec.dir, d.sha256, d.file_count, d.byte_size),
                Err(e) => {
                    eprintln!("ERROR: {}: cannot hash {}: {e}", spec.id, dir.display());
                    return 3;
                }
            }
        }
        println!("scenes={} seed={}", emitted.len(), seed);
        return 0;
    }

    verify_freeze(&out, seed, &emitted, record.as_deref().unwrap_or(Path::new("scene_freeze.json")))
}

/// Re-emit every scene into a temporary directory, hash it and the tree under
/// `--out`, compare scene by scene, write the freeze record, read it back and
/// re-check every recorded hash against the disk.
fn verify_freeze(
    out: &Path,
    seed: u64,
    emitted: &[(&SceneSpec, BTreeMap<String, Vec<u8>>)],
    record_path: &Path,
) -> i32 {
    let temp = std::env::temp_dir().join(format!("harness-scenes-verify-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    let generator_sha = hex(&Sha256::digest(GENERATOR_SOURCE));
    let (commit, source_clean) = git_state();

    let mut rows = Vec::new();
    let mut identical = 0usize;
    let mut differing = 0usize;
    for (spec, files) in emitted {
        let fresh_dir = temp.join(spec.dir);
        if let Err(e) = write_scene_dir(&fresh_dir, files, true) {
            eprintln!("ERROR: {}: cannot write the re-emission: {e}", spec.id);
            let _ = std::fs::remove_dir_all(&temp);
            return 3;
        }
        let fresh = match tree_digest(&fresh_dir) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("ERROR: {}: cannot hash the re-emission: {e}", spec.id);
                let _ = std::fs::remove_dir_all(&temp);
                return 3;
            }
        };
        let on_disk = tree_digest(&out.join(spec.dir));
        let (verdict, disk_sha, diff_files, count, bytes) = match &on_disk {
            Ok(d) if d.sha256 == fresh.sha256 => ("identical", d.sha256.clone(), Vec::new(), d.file_count, d.byte_size),
            Ok(d) => ("differing", d.sha256.clone(), diff_file_list(d, &fresh), d.file_count, d.byte_size),
            Err(_) => ("missing", String::new(), Vec::new(), 0, 0),
        };
        if verdict == "identical" {
            identical += 1;
        } else {
            differing += 1;
        }
        println!("{} {:<20} sha256:{}  {}", spec.id, spec.dir, if disk_sha.is_empty() { "-" } else { &disk_sha }, verdict);
        for f in diff_files.iter().take(20) {
            println!("    differs: {f}");
        }
        rows.push(serde_json::json!({
            "scene_id": spec.id,
            "space_dir": spec.dir,
            "space_path": format!("eustress/spaces/harness/{}", spec.dir),
            "title": spec.title,
            "sha256": if verdict == "identical" { serde_json::Value::String(disk_sha.clone()) } else { serde_json::Value::Null },
            "sha256_on_disk": if disk_sha.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(disk_sha.clone()) },
            "sha256_regenerated": fresh.sha256,
            "file_count": count,
            "byte_size": bytes,
            "seed": seed,
            "generator_sha256": generator_sha,
            "commit": commit,
            "verdict": verdict,
            "differing_files": diff_files,
        }));
    }
    let _ = std::fs::remove_dir_all(&temp);
    println!("scenes={} identical={} differing={}", emitted.len(), identical, differing);

    // A frozen scene id never changes hash: refuse to overwrite a record that
    // already freezes a different hash under the same id and seed.
    if let Ok(text) = std::fs::read_to_string(record_path) {
        if let Ok(prev) = serde_json::from_str::<serde_json::Value>(&text) {
            let mut changed = Vec::new();
            for old in prev.get("scenes").and_then(|s| s.as_array()).into_iter().flatten() {
                let (Some(id), Some(old_sha)) = (old.get("scene_id").and_then(|v| v.as_str()), old.get("sha256").and_then(|v| v.as_str())) else { continue };
                if old.get("seed").and_then(|v| v.as_u64()) != Some(seed) {
                    continue;
                }
                if let Some(new) = rows.iter().find(|r| r["scene_id"] == id) {
                    if new["sha256_regenerated"].as_str() != Some(old_sha) {
                        changed.push(format!("{id}: frozen {old_sha}, now {}", new["sha256_regenerated"].as_str().unwrap_or("-")));
                    }
                }
            }
            if !changed.is_empty() {
                for c in &changed {
                    eprintln!("FROZEN-HASH-CHANGED {c}");
                }
                eprintln!("ERROR: a frozen scene changed. Give it a new scene id (S1 becomes S1b) instead of re-freezing it; the record was left as it was.");
                return 4;
            }
        }
    }

    let record = serde_json::json!({
        "item": "G1.03",
        "artifact": "docs/PROMPTS/artifacts/G1.03/scene_freeze.json",
        "record_version": 1,
        "generator": {
            "bin": BIN_NAME,
            "source_path": GENERATOR_REL_PATH,
            "source_sha256": generator_sha,
            "commit": commit,
            "source_matches_commit": source_clean,
        },
        "seed": seed,
        "hash_method": "SHA-256 over every regular file in byte order of its '/'-separated Space-relative path: b\"F\\0\" + path + b\"\\0\" + u64-LE length + SHA-256(bytes). Directories carry no bytes.",
        "regeneration": "the verifying run re-emitted every scene into a temporary directory and compared its tree hash with the tree under --out written by an earlier run",
        "recorded_at_unix_s": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        "scenes": rows,
        "summary": { "scenes": emitted.len(), "identical": identical, "differing": differing },
    });
    if let Some(parent) = record_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let text = match serde_json::to_string_pretty(&record) {
        Ok(t) => t + "\n",
        Err(e) => {
            eprintln!("ERROR: cannot serialise the freeze record: {e}");
            return 5;
        }
    };
    if let Err(e) = std::fs::write(record_path, text) {
        eprintln!("ERROR: cannot write {}: {e}", record_path.display());
        return 5;
    }

    // Read the record back and recompute every recorded hash from disk.
    let mut readback_mismatches = 0usize;
    match std::fs::read_to_string(record_path).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
        Some(back) => {
            for row in back.get("scenes").and_then(|s| s.as_array()).into_iter().flatten() {
                let dir = row.get("space_dir").and_then(|v| v.as_str()).unwrap_or("");
                let recorded = row.get("sha256").and_then(|v| v.as_str());
                let now = tree_digest(&out.join(dir)).ok().map(|d| d.sha256);
                if recorded.is_none() || recorded != now.as_deref() {
                    readback_mismatches += 1;
                    println!("readback: {dir} recorded={} disk={}", recorded.unwrap_or("null"), now.as_deref().unwrap_or("missing"));
                }
            }
        }
        None => readback_mismatches = emitted.len(),
    }
    println!("record={} readback_mismatches={}", record_path.display(), readback_mismatches);

    if differing == 0 && readback_mismatches == 0 && emitted.len() == SCENES.len() {
        0
    } else {
        1
    }
}

fn diff_file_list(a: &TreeDigest, b: &TreeDigest) -> Vec<String> {
    let mut out = Vec::new();
    for (p, h) in &a.files {
        match b.files.get(p) {
            Some(h2) if h2 == h => {}
            Some(_) => out.push(format!("{p} (content)")),
            None => out.push(format!("{p} (only on disk)")),
        }
    }
    for p in b.files.keys() {
        if !a.files.contains_key(p) {
            out.push(format!("{p} (only in the re-emission)"));
        }
    }
    out
}

fn git_state() -> (serde_json::Value, serde_json::Value) {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(manifest_dir)
            .args(args)
            .output()
            .ok()
    };
    let commit = run(&["rev-parse", "HEAD"])
        .filter(|o| o.status.success())
        .map(|o| serde_json::Value::String(String::from_utf8_lossy(&o.stdout).trim().to_string()))
        .unwrap_or(serde_json::Value::Null);
    let src = "src/bin/harness_scenes.rs";
    let tracked = run(&["ls-files", "--error-unmatch", src]).map(|o| o.status.success());
    let clean = run(&["diff", "--quiet", "HEAD", "--", src]).map(|o| o.status.success());
    let state = match (tracked, clean) {
        (Some(t), Some(c)) => serde_json::Value::Bool(t && c),
        _ => serde_json::Value::Null,
    };
    (commit, state)
}

// ─────────────────────────────────────────────────────────────────────────────
// Writing and hashing
// ─────────────────────────────────────────────────────────────────────────────

/// Marker every generated scene carries; `--replace` only deletes a folder
/// that holds it.
const MARKER: &str = ".eustress/harness/scene.toml";

fn write_scene_dir(dir: &Path, files: &BTreeMap<String, Vec<u8>>, replace: bool) -> Result<(), String> {
    if dir.exists() {
        let existing = tree_digest(dir)?;
        let extra: Vec<&String> = existing.files.keys().filter(|p| !files.contains_key(*p)).collect();
        if !extra.is_empty() {
            if !replace {
                let shown: Vec<&str> = extra.iter().take(10).map(|s| s.as_str()).collect();
                return Err(format!(
                    "{} holds {} file(s) this generator does not emit (first: {}). It was probably opened in the engine; pass --replace to remove and re-emit it",
                    dir.display(),
                    extra.len(),
                    shown.join(", ")
                ));
            }
            if !dir.join(MARKER).is_file() {
                return Err(format!("{} has no {MARKER}; refusing to delete a folder this generator did not write", dir.display()));
            }
            std::fs::remove_dir_all(dir).map_err(|e| format!("remove {}: {e}", dir.display()))?;
        }
    }
    for (rel, bytes) in files {
        let path = rel.split('/').fold(dir.to_path_buf(), |p, seg| p.join(seg));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    Ok(())
}

struct TreeDigest {
    sha256: String,
    files: BTreeMap<String, String>,
    file_count: usize,
    byte_size: u64,
}

fn tree_digest(root: &Path) -> Result<TreeDigest, String> {
    if !root.is_dir() {
        return Err(format!("{} is not a directory", root.display()));
    }
    let mut all: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    collect_files(root, "", &mut all)?;
    let mut outer = Sha256::new();
    let mut files = BTreeMap::new();
    let mut byte_size = 0u64;
    for (rel, bytes) in &all {
        let file_sha = Sha256::digest(bytes);
        outer.update(b"F\0");
        outer.update(rel.as_bytes());
        outer.update(b"\0");
        outer.update((bytes.len() as u64).to_le_bytes());
        outer.update(file_sha);
        files.insert(rel.clone(), hex(&file_sha));
        byte_size += bytes.len() as u64;
    }
    Ok(TreeDigest { sha256: hex(&outer.finalize()), file_count: files.len(), files, byte_size })
}

fn collect_files(dir: &Path, prefix: &str, out: &mut BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let ft = entry.file_type().map_err(|e| format!("stat {rel}: {e}"))?;
        if ft.is_dir() {
            collect_files(&entry.path(), &rel, out)?;
        } else if ft.is_file() {
            let bytes = std::fs::read(entry.path()).map_err(|e| format!("read {rel}: {e}"))?;
            out.insert(rel, bytes);
        } else {
            return Err(format!("{rel} is neither a file nor a directory (symlink?)"));
        }
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Deterministic helpers
// ─────────────────────────────────────────────────────────────────────────────

/// SplitMix64: a seed of 0 still produces a full-period stream.
struct Rng(u64);

impl Rng {
    fn new(seed: u64, scene_id: &str) -> Self {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in scene_id.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Rng(seed ^ h)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

/// Round to 6 decimals, with -0.0 folded to 0.0.
fn r6(x: f64) -> f64 {
    let v = (x * 1.0e6).round() / 1.0e6;
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

/// A TOML float literal (always carries a decimal point).
fn ff(x: f64) -> String {
    format!("{:?}", r6(x))
}

fn v3(v: [f64; 3]) -> String {
    format!("[{}, {}, {}]", ff(v[0]), ff(v[1]), ff(v[2]))
}

fn v4(v: [f64; 4]) -> String {
    format!("[{}, {}, {}, {}]", ff(v[0]), ff(v[1]), ff(v[2]), ff(v[3]))
}

const IDENTITY: [f64; 4] = [0.0, 0.0, 0.0, 1.0];

/// The rotation whose -Z axis points along `dir` (Bevy's `looking_to` with +Y up).
fn look_rotation(dir: [f64; 3]) -> [f64; 4] {
    let n = |v: [f64; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let f = n(dir);
    let z = [-f[0], -f[1], -f[2]];
    let x = n(cross([0.0, 1.0, 0.0], z));
    let y = cross(z, x);
    let (m00, m01, m02) = (x[0], y[0], z[0]);
    let (m10, m11, m12) = (x[1], y[1], z[1]);
    let (m20, m21, m22) = (x[2], y[2], z[2]);
    let tr = m00 + m11 + m22;
    let q = if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [(m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [(m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [(m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s]
    };
    [r6(q[0]), r6(q[1]), r6(q[2]), r6(q[3])]
}

/// Direction light travels for a source at `elevation_deg` above the horizon
/// and `azimuth_deg` clockwise from north (-Z) toward east (+X).
fn sun_travel_dir(elevation_deg: f64, azimuth_deg: f64) -> [f64; 3] {
    let (e, a) = (elevation_deg.to_radians(), azimuth_deg.to_radians());
    let to_sun = [a.sin() * e.cos(), e.sin(), -a.cos() * e.cos()];
    [r6(-to_sun[0]), r6(-to_sun[1]), r6(-to_sun[2])]
}

// ─────────────────────────────────────────────────────────────────────────────
// Scene builder
// ─────────────────────────────────────────────────────────────────────────────

struct Scene {
    id: &'static str,
    dir: &'static str,
    title: &'static str,
    seed: u64,
    rng: Rng,
    files: BTreeMap<String, String>,
    /// Lighting service and children, set by each scene before `finish`.
    lighting: LightingSetup,
    simulation_toml: Option<String>,
    harness_notes: BTreeMap<String, String>,
}

#[derive(Clone, Copy)]
struct LightingSetup {
    ambient: [f64; 3],
    outdoor_ambient: [f64; 3],
    environment_diffuse_scale: f64,
    environment_specular_scale: f64,
    clock_time: f64,
    sun_enabled: bool,
    atmosphere_environment: bool,
    celestial_bodies_shown: bool,
}

impl Default for LightingSetup {
    fn default() -> Self {
        Self {
            ambient: [0.0, 0.0, 0.0],
            outdoor_ambient: [0.5, 0.5, 0.5],
            environment_diffuse_scale: 1.0,
            environment_specular_scale: 1.0,
            clock_time: 14.0,
            sun_enabled: true,
            atmosphere_environment: true,
            celestial_bodies_shown: true,
        }
    }
}

#[derive(Clone)]
struct Part {
    path: String,
    mesh: &'static str,
    pos: [f64; 3],
    rot: [f64; 4],
    size: [f64; 3],
    color: [u8; 3],
    material: String,
    anchored: bool,
    can_collide: bool,
    cast_shadow: bool,
    transparency: f64,
    density: Option<f64>,
    restitution: Option<f64>,
    friction: Option<f64>,
    attributes: Vec<(String, String)>,
}

impl Part {
    fn block(path: impl Into<String>, pos: [f64; 3], size: [f64; 3]) -> Self {
        Part {
            path: path.into(),
            mesh: "parts/block.glb",
            pos,
            rot: IDENTITY,
            size,
            color: [163, 162, 165],
            material: "SmoothPlastic".to_string(),
            anchored: true,
            can_collide: true,
            cast_shadow: true,
            transparency: 0.0,
            density: None,
            restitution: None,
            friction: None,
            attributes: Vec::new(),
        }
    }
    fn ball(path: impl Into<String>, pos: [f64; 3], diameter: f64) -> Self {
        Part { mesh: "parts/ball.glb", ..Part::block(path, pos, [diameter, diameter, diameter]) }
    }
    fn color(mut self, c: [u8; 3]) -> Self {
        self.color = c;
        self
    }
    fn material(mut self, m: &str) -> Self {
        self.material = m.to_string();
        self
    }
    fn dynamic(mut self) -> Self {
        self.anchored = false;
        self
    }
    fn attr(mut self, k: &str, v: String) -> Self {
        self.attributes.push((k.to_string(), v));
        self
    }
}

fn q(s: &str) -> String {
    format!("{s:?}")
}

impl Scene {
    fn new(spec: &SceneSpec, seed: u64) -> Self {
        Scene {
            id: spec.id,
            dir: spec.dir,
            title: spec.title,
            seed,
            rng: Rng::new(seed, spec.id),
            files: BTreeMap::new(),
            lighting: LightingSetup::default(),
            simulation_toml: None,
            harness_notes: BTreeMap::new(),
        }
    }

    /// Every text file is stored with LF line endings, whatever line endings
    /// the checkout gave an embedded template, so the bytes do not depend on
    /// the machine's git settings.
    fn put(&mut self, rel: &str, text: String) {
        let text = text.replace("\r\n", "\n");
        let text = if text.ends_with('\n') { text } else { text + "\n" };
        self.files.insert(rel.to_string(), text);
    }

    /// 32 lowercase hex characters, derived from the seed, the Space and the
    /// instance's path: stable across runs, distinct per instance.
    fn uuid(&self, instance_path: &str) -> String {
        let d = Sha256::digest(format!("eustress-harness|{}|{}|{}", self.dir, self.seed, instance_path).as_bytes());
        hex(&d[..16])
    }

    fn folder(&mut self, path: &str) {
        let name = path.rsplit('/').next().unwrap_or(path);
        let text = format!(
            "[metadata]\nclass_name = \"Folder\"\narchivable = true\nname = {}\nuuid = {}\n",
            q(name),
            q(&self.uuid(path))
        );
        self.put(&format!("{path}/_instance.toml"), text);
    }

    fn part(&mut self, p: Part) -> String {
        let name = p.path.rsplit('/').next().unwrap_or(&p.path).to_string();
        let uuid = self.uuid(&p.path);
        let mut t = String::new();
        t.push_str(&format!(
            "[metadata]\nclass_name = \"Part\"\narchivable = true\nname = {}\nuuid = {}\nunit = \"m\"\ncreated = {}\nlast_modified = {}\n\n",
            q(&name),
            q(&uuid),
            q(GENERATOR_EPOCH),
            q(GENERATOR_EPOCH)
        ));
        t.push_str(&format!("[asset]\nmesh = {}\nscene = \"Scene0\"\n\n", q(p.mesh)));
        t.push_str(&format!("[transform]\nposition = {}\nrotation = {}\nscale = {}\n\n", v3(p.pos), v4(p.rot), v3(p.size)));
        t.push_str(&format!(
            "[properties]\ncolor = [{}, {}, {}]\ntransparency = {}\nanchored = {}\ncan_collide = {}\ncast_shadow = {}\nreflectance = 0.0\nmaterial = {}\nlocked = false\n",
            p.color[0], p.color[1], p.color[2], ff(p.transparency), p.anchored, p.can_collide, p.cast_shadow, q(&p.material)
        ));
        if p.density.is_some() || p.restitution.is_some() || p.friction.is_some() {
            t.push_str("\n[properties.physics]\n");
            if let Some(d) = p.density {
                t.push_str(&format!("density = {}\ndensity_unit = \"kg/m3\"\n", ff(d)));
            }
            if let Some(r) = p.restitution {
                t.push_str(&format!("restitution = {}\n", ff(r)));
            }
            if let Some(f) = p.friction {
                t.push_str(&format!("friction_static = {}\nfriction_kinetic = {}\n", ff(f), ff(f)));
            }
        }
        if !p.attributes.is_empty() {
            t.push_str("\n[attributes]\n");
            let sorted: BTreeMap<_, _> = p.attributes.iter().cloned().collect();
            for (k, v) in sorted {
                t.push_str(&format!("{k} = {v}\n"));
            }
        }
        self.put(&format!("{}/_instance.toml", p.path), t);
        uuid
    }

    /// A light hung inside a part's folder as a child FOLDER with its own
    /// `_instance.toml` (`<Part>/<Name>/_instance.toml`). A flat
    /// `<Name>.instance.toml` inside a Part folder is never spawned: the
    /// loader skips every file child of a Part folder
    /// (`file_loader.rs` `part_subfolders_only`) and descends only into
    /// directories.
    fn light(&mut self, parent: &str, name: &str, class: &str, color: [u8; 3], brightness: f64, range: f64, extra: &str) {
        let path = format!("{parent}/{name}");
        let text = format!(
            "[metadata]\nclass_name = {}\narchivable = true\nname = {}\nuuid = {}\n\n[light]\ncolor = [{}, {}, {}]\nbrightness = {}\nrange = {}\nshadows = true\nenabled = true\n{}",
            q(class),
            q(name),
            q(&self.uuid(&path)),
            color[0],
            color[1],
            color[2],
            ff(brightness),
            ff(range),
            extra
        );
        self.put(&format!("{path}/_instance.toml"), text);
    }

    /// A lamp: a small anchored, non-colliding emitter part with a light in it.
    fn lamp(&mut self, path: &str, pos: [f64; 3], class: &str, color: [u8; 3], brightness: f64, range: f64, extra: &str) {
        let mut p = Part::ball(path, pos, 0.1).color(color).material("H_Emissive");
        p.can_collide = false;
        p.cast_shadow = false;
        self.part(p);
        self.light(path, "Light", class, color, brightness, range, extra);
    }

    fn directional_light(&mut self, name: &str, dir: [f64; 3], color: [u8; 3], brightness: f64) {
        let path = format!("Lighting/{name}");
        let text = format!(
            "[metadata]\nclass_name = \"DirectionalLight\"\narchivable = true\nname = {}\nuuid = {}\n\n[transform]\nposition = [0.0, 50.0, 0.0]\nrotation = {}\nscale = [1.0, 1.0, 1.0]\n\n[light]\ncolor = [{}, {}, {}]\nbrightness = {}\nshadows = true\nenabled = true\n",
            q(name),
            q(&self.uuid(&path)),
            v4(look_rotation(dir)),
            color[0],
            color[1],
            color[2],
            ff(brightness)
        );
        self.put(&format!("{path}.instance.toml"), text);
    }

    fn material(&mut self, name: &str, description: &str, pbr: &str) {
        let text = format!(
            "[material]\nname = {}\ndescription = {}\n\n[pbr]\nbase_color = [1.0, 1.0, 1.0, 1.0]\n{}",
            q(name),
            q(description),
            pbr
        );
        self.put(&format!("MaterialService/{name}.mat.toml"), text);
    }

    fn note(&mut self, key: &str, value: String) {
        self.harness_notes.insert(key.to_string(), value);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Files every harness Space carries
// ─────────────────────────────────────────────────────────────────────────────

macro_rules! svc {
    ($name:literal) => {
        ($name, include_str!(concat!("../../../common/assets/service_templates/", $name, "/_service.toml")))
    };
}

/// Service folders with a shipped template, as `space_ops::SERVICE_FOLDERS`
/// copies them (Lighting is written per scene below).
const SERVICE_TEMPLATES: &[(&str, &str)] = &[
    svc!("Workspace"),
    svc!("Players"),
    svc!("StarterGui"),
    svc!("StarterPack"),
    svc!("ReplicatedStorage"),
    svc!("ServerStorage"),
    svc!("ServerScriptService"),
    svc!("SoulService"),
    svc!("MaterialService"),
    svc!("SoundService"),
    svc!("PhysicsService"),
    svc!("AdornmentService"),
    svc!("DataService"),
    svc!("Website"),
    svc!("ExperimentService"),
    svc!("Teams"),
    svc!("Chat"),
];

/// Services in `SERVICE_FOLDERS` with no template: the integrity repair would
/// write them with a wall-clock stamp, so they are emitted here with a fixed one.
const SERVICES_WITHOUT_TEMPLATE: &[(&str, &str, &str)] = &[
    ("StarterPlayerScripts", "starterplayer", "Scripts cloned into each player on join"),
    ("StarterCharacterScripts", "starterplayer", "Scripts cloned into each character on spawn"),
];

macro_rules! mat {
    ($name:literal) => {
        ($name, include_str!(concat!("../../../common/assets/service_templates/MaterialService/", $name, ".mat.toml")))
    };
}

const LIBRARY_MATERIALS: &[(&str, &str)] = &[
    mat!("Brick"),
    mat!("Bronze"),
    mat!("Concrete"),
    mat!("CorrodedMetal"),
    mat!("DiamondPlate"),
    mat!("Fabric"),
    mat!("Foil"),
    mat!("Glass"),
    mat!("Gold"),
    mat!("Granite"),
    mat!("Grass"),
    mat!("Ice"),
    mat!("Marble"),
    mat!("Metal"),
    mat!("Neon"),
    mat!("Plastic"),
    mat!("Sand"),
    mat!("Silver"),
    mat!("Slate"),
    mat!("SmoothPlastic"),
    mat!("Wood"),
    mat!("WoodPlanks"),
];

const SUN_TEMPLATE: &str = include_str!("../../assets/lighting_templates/Sun.instance.toml");
const MOON_TEMPLATE: &str = include_str!("../../assets/lighting_templates/Moon.instance.toml");
const SKY_TEMPLATE: &str = include_str!("../../assets/lighting_templates/Sky.instance.toml");
const ATMOSPHERE_TEMPLATE: &str = include_str!("../../assets/lighting_templates/Atmosphere.instance.toml");

/// Replace `from` with `to` exactly once; a template that drifted fails the
/// emission instead of silently keeping the old value.
fn set_once(text: &str, from: &str, to: &str) -> Result<String, String> {
    match text.matches(from).count() {
        1 => Ok(text.replacen(from, to, 1)),
        n => Err(format!("template line {from:?} found {n} times, expected once")),
    }
}

impl Scene {
    fn finish(mut self) -> Result<BTreeMap<String, Vec<u8>>, String> {
        let dir = self.dir;

        // space.toml: name equals the folder name, so the open path's name
        // sync leaves it alone; format is the build's current Space format.
        self.put(
            "space.toml",
            format!(
                "# EEP Space metadata (emitted by {BIN_NAME}; never edit by hand)\n[space]\nname = {}\nauthor = \"{BIN_NAME}\"\nversion = \"0.1.0\"\ncreated_with = \"{BIN_NAME}\"\nformat = {}\n\n[metadata]\ncreated = {}\nlast_modified = {}\n",
                q(dir),
                eustress_common::space_format::CURRENT,
                q(GENERATOR_EPOCH),
                q(GENERATOR_EPOCH)
            ),
        );
        let sim = self.simulation_toml.take().unwrap_or_else(|| simulation_toml(false, false, ""));
        self.put("simulation.toml", sim);

        // .eustress manifests the open path would otherwise stamp with now().
        self.put(
            ".eustress/project.toml",
            format!(
                "[project]\nname = {}\nversion = \"0.1.0\"\nauthor = \"{BIN_NAME}\"\neustress_version = \"{BIN_NAME}\"\ncreated = {}\nlast_opened = {}\n\n[format]\neep_version = \"1.0\"\nscene_format = \"toml\"\n",
                q(dir),
                q(GENERATOR_EPOCH),
                q(GENERATOR_EPOCH)
            ),
        );
        self.put(
            ".eustress/settings.toml",
            "[editor]\nshow_grid = true\ngrid_size = 1.0\nsnap_enabled = true\nsnap_size = 1.0\ntheme = \"dark\"\n\n[camera]\nfov = 70.0\nnear = 0.1\nfar = 10000.0\nmove_speed = 10.0\n\n[rendering]\nshadows = true\nambient_occlusion = false\n\n[sync]\nmode = \"local_first\"\nauto_sync = false\nprewarm_packages = true\nprewarm_assets = true\n".to_string(),
        );
        self.put(
            ".eustress/sync.toml",
            "[sync]\nmode = \"local_first\"\nauto_sync = false\npending_uploads = 0\npending_downloads = 0\noffline_changes = 0\n\n[remote]\nprovider = \"cloudflare_r2\"\neditable = false\nopen_source = false\n".to_string(),
        );
        self.put(".eustress/asset-index.toml", "assets = []\n".to_string());
        self.put(".eustress/package-index.toml", "packages = []\n".to_string());
        self.put(
            ".eustress/publish.toml",
            "releases = []\n\n[publish]\nversion = 1\nchannel = \"production\"\n\n[listing]\nname = \"\"\ngenre = \"All\"\n\n[visibility]\nis_public = false\nopen_source = false\nstudio_editable = false\ndiscoverable = false\nmax_players = 10\n".to_string(),
        );
        let mut journal = format!("[journal]\nstage = \"idle\"\nupdated_at = {}\nresumable = true\n", q(GENERATOR_EPOCH));
        for name in ["scan", "package", "upload", "commit"] {
            journal.push_str(&format!("\n[[checkpoints]]\nname = \"{name}\"\ncompleted = false\nupdated_at = {}\n", q(GENERATOR_EPOCH)));
        }
        self.put(".eustress/publish-journal.toml", journal);
        // Written on every open with the folder name: identical bytes.
        self.files.insert(".eustress/.last_name".to_string(), dir.to_string());

        // Harness marker and per-scene notes.
        let mut marker = format!(
            "# Harness scene marker. Emitted by {BIN_NAME} ({GENERATOR_REL_PATH}); never edit by hand.\n[harness]\nscene_id = {}\nspace_dir = {}\nseed = {}\ngenerator = \"{BIN_NAME}\"\ntitle = {}\n",
            q(self.id),
            q(dir),
            self.seed,
            q(self.title)
        );
        if !self.harness_notes.is_empty() {
            marker.push_str("\n[notes]\n");
            for (k, v) in &self.harness_notes {
                marker.push_str(&format!("{k} = {}\n", q(v)));
            }
        }
        self.put(MARKER, marker);

        // Services.
        for (name, text) in SERVICE_TEMPLATES {
            self.put(&format!("{name}/_service.toml"), text.to_string());
        }
        for (name, icon, description) in SERVICES_WITHOUT_TEMPLATE {
            self.put(
                &format!("{name}/_service.toml"),
                format!(
                    "# EEP _service.toml: marks this folder as a Service container.\n[service]\nclass_name = \"{name}\"\nicon = \"{icon}\"\ndescription = \"{description}\"\ncan_have_children = true\n\n[metadata]\nid = \"{}-service\"\ncreated = {}\nlast_modified = {}\n",
                    name.to_lowercase(),
                    q(GENERATOR_EPOCH),
                    q(GENERATOR_EPOCH)
                ),
            );
        }
        for (name, text) in LIBRARY_MATERIALS {
            self.put(&format!("MaterialService/{name}.mat.toml"), text.to_string());
        }

        // Lighting service and its four children.
        let l = self.lighting;
        let c = |v: [f64; 3]| v4([v[0], v[1], v[2], 1.0]);
        let lighting = format!(
            "# Lighting Service (harness scene {}): fixed clock, no day cycle.\n\n[service]\nclass_name = \"Lighting\"\nicon = \"lighting\"\ndescription = \"Controls global lighting, shadows, and atmospheric effects\"\ncan_have_children = true\n\n[properties]\nambient = {}\noutdoor_ambient = {}\nbrightness = 2.0\ncolor_shift_top = [0.0, 0.0, 0.0, 1.0]\ncolor_shift_bottom = [0.0, 0.0, 0.0, 1.0]\nenvironment_diffuse_scale = {}\nenvironment_specular_scale = {}\nclock_time = {}\ngeographic_latitude = 41.73\nfog_color = [0.75, 0.75, 0.75, 1.0]\nfog_end = 100000.0\nfog_start = 0.0\nglobal_shadows = true\nexposure_compensation = 0.0\nday_cycle = false\nday_length_minutes = 24.0\n\n[metadata]\nid = \"lighting-service\"\ncreated = {}\nlast_modified = {}\n",
            self.id,
            c(l.ambient),
            c(l.outdoor_ambient),
            ff(l.environment_diffuse_scale),
            ff(l.environment_specular_scale),
            ff(l.clock_time),
            q(GENERATOR_EPOCH),
            q(GENERATOR_EPOCH)
        );
        self.put("Lighting/_service.toml", lighting);
        let sun = set_once(SUN_TEMPLATE, "enabled = true", &format!("enabled = {}", l.sun_enabled))?;
        // The Moon is off in every harness scene: `follows_calendar` places it
        // by today's date, which no frozen scene may depend on.
        let moon = set_once(MOON_TEMPLATE, "enabled = true", "enabled = false")?;
        let moon = set_once(&moon, "follows_calendar = true", "follows_calendar = false")?;
        let sky = set_once(SKY_TEMPLATE, "celestial_bodies_shown = true", &format!("celestial_bodies_shown = {}", l.celestial_bodies_shown))?;
        let mut atmosphere = ATMOSPHERE_TEMPLATE.to_string();
        if !l.atmosphere_environment {
            atmosphere = set_once(&atmosphere, "environment_map_enabled = true", "environment_map_enabled = false")?;
            atmosphere = set_once(&atmosphere, "environment_intensity = 1.0", "environment_intensity = 0.0")?;
            atmosphere = set_once(&atmosphere, "atmosphere_environment_light = true", "atmosphere_environment_light = false")?;
        }
        self.put("Lighting/Sun.instance.toml", sun);
        self.put("Lighting/Moon.instance.toml", moon);
        self.put("Lighting/Sky.instance.toml", sky);
        self.put("Lighting/Atmosphere.instance.toml", atmosphere);

        // Canonicalise every instance file to the form the loader's self-heal
        // converges on, and prove that form is a fixed point.
        let registry = eustress_common::class_schema::ClassSchemaRegistry::builtin();
        let mut out = BTreeMap::new();
        for (rel, text) in std::mem::take(&mut self.files) {
            let is_instance = rel.ends_with("/_instance.toml") || rel.ends_with(".instance.toml");
            let bytes = if is_instance {
                let canonical = canonical_instance(&text, registry).map_err(|e| format!("{rel}: {e}"))?;
                let again = canonical_instance(&canonical, registry).map_err(|e| format!("{rel} (second pass): {e}"))?;
                if again != canonical {
                    return Err(format!(
                        "HEAL_NOT_IDEMPOTENT {rel}: the engine's self-heal does not reach a fixed point on this file, so the loaded tree would differ from the hashed tree"
                    ));
                }
                canonical.into_bytes()
            } else {
                text.into_bytes()
            };
            out.insert(rel, bytes);
        }
        Ok(out)
    }
}

/// The text `class_schema::load_and_heal_instance` would leave on disk.
fn canonical_instance(text: &str, registry: &eustress_common::class_schema::ClassSchemaRegistry) -> Result<String, String> {
    let healed = eustress_common::class_schema::heal_instance_from_str(text, registry)?;
    toml::to_string_pretty(&healed.value).map_err(|e| format!("reserialise: {e}"))
}

fn simulation_toml(auto_start: bool, recording: bool, extra: &str) -> String {
    format!(
        "# Simulation configuration (harness scene). Tick rate equals the engine's fixed step.\n\n[simulation]\ntick_rate_hz = 60.0\ntime_scale = 1.0\nmax_ticks_per_frame = 10\nauto_start = {auto_start}\n\n[simulation.recording]\nenabled = {recording}\noutput_dir = \".eustress/knowledge/recordings\"\nformat = \"both\"\nauto_export = {recording}\n{extra}"
    )
}

fn emit_scene(spec: &SceneSpec, seed: u64) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut scene = Scene::new(spec, seed);
    (spec.build)(&mut scene);
    scene.finish()
}

// ─────────────────────────────────────────────────────────────────────────────
// S1: sphere grid
// ─────────────────────────────────────────────────────────────────────────────

/// sRGB patch values of the 24-patch chart (CONFIG DEFAULT, the commonly
/// published sRGB approximations of the ColorChecker Classic).
const CHART: [(&str, [u8; 3]); 24] = [
    ("dark_skin", [115, 82, 68]),
    ("light_skin", [194, 150, 130]),
    ("blue_sky", [98, 122, 157]),
    ("foliage", [87, 108, 67]),
    ("blue_flower", [133, 128, 177]),
    ("bluish_green", [103, 189, 170]),
    ("orange", [214, 126, 44]),
    ("purplish_blue", [80, 91, 166]),
    ("moderate_red", [193, 90, 99]),
    ("purple", [94, 60, 108]),
    ("yellow_green", [157, 188, 64]),
    ("orange_yellow", [224, 163, 46]),
    ("blue", [56, 61, 150]),
    ("green", [70, 148, 73]),
    ("red", [175, 54, 60]),
    ("yellow", [231, 199, 31]),
    ("magenta", [187, 86, 149]),
    ("cyan", [8, 133, 161]),
    ("white", [243, 243, 242]),
    ("neutral_8", [200, 200, 200]),
    ("neutral_6_5", [160, 160, 160]),
    ("neutral_5", [122, 122, 121]),
    ("neutral_3_5", [85, 85, 85]),
    ("black", [52, 52, 52]),
];

/// 18% linear reflectance in 8-bit sRGB.
const GREY_18: [u8; 3] = [118, 118, 118];

fn common_materials(s: &mut Scene) {
    s.material("H_Matte", "Harness: fully rough dielectric", "metallic = 0.0\nroughness = 1.0\nreflectance = 0.5\n");
    s.material("H_Emissive", "Harness: emissive surface", "metallic = 0.0\nroughness = 0.5\nreflectance = 0.5\nemissive = [4.0, 4.0, 4.0]\n");
}

fn build_s1(s: &mut Scene) {
    common_materials(s);
    s.lighting = LightingSetup {
        ambient: [0.0, 0.0, 0.0],
        outdoor_ambient: [0.0, 0.0, 0.0],
        environment_diffuse_scale: 0.0,
        environment_specular_scale: 0.0,
        sun_enabled: false,
        atmosphere_environment: false,
        celestial_bodies_shown: false,
        ..LightingSetup::default()
    };
    s.note("lights", "three PointLights only (key, fill, rim); sun, moon and sky light off".to_string());

    s.part(Part::block("Workspace/Ground", [0.0, -0.5, 0.0], [40.0, 1.0, 40.0]).color(GREY_18).material("H_Matte"));

    // 8 columns: roughness 0..1. 5 rows: metallic 0..1.
    s.folder("Workspace/SphereGrid");
    let (cols, rows) = (8usize, 5usize);
    let spacing = 1.0;
    let diameter = 0.8;
    for row in 0..rows {
        for col in 0..cols {
            let roughness = col as f64 / (cols - 1) as f64;
            let metallic = row as f64 / (rows - 1) as f64;
            let mat = format!("H_R{col}_M{row}");
            s.material(
                &mat,
                &format!("Harness S1 grid: roughness {} metallic {}", ff(roughness), ff(metallic)),
                &format!("metallic = {}\nroughness = {}\nreflectance = 0.5\n", ff(metallic), ff(roughness)),
            );
            let x = (col as f64 - (cols - 1) as f64 / 2.0) * spacing;
            let z = (row as f64 - (rows - 1) as f64 / 2.0) * spacing;
            s.part(
                Part::ball(format!("Workspace/SphereGrid/Sphere_c{col}_r{row}"), [x, diameter / 2.0, z], diameter)
                    .color([204, 204, 204])
                    .material(&mat)
                    .attr("roughness", ff(roughness))
                    .attr("metallic", ff(metallic)),
            );
        }
    }

    // Calibration chart: 6 x 4 patches on a matte black board, facing +Z.
    s.folder("Workspace/CalibrationChart");
    let board_center = [0.0, 1.0, -4.0];
    s.part(Part::block("Workspace/CalibrationChart/Board", board_center, [2.0, 1.4, 0.05]).color([20, 20, 20]).material("H_Matte"));
    for (i, (name, rgb)) in CHART.iter().enumerate() {
        let (cx, cy) = ((i % 6) as f64, (i / 6) as f64);
        let pos = [board_center[0] - 0.75 + cx * 0.3, board_center[1] + 0.45 - cy * 0.3, board_center[2] + 0.03];
        s.part(
            Part::block(format!("Workspace/CalibrationChart/Patch_{:02}_{name}", i + 1), pos, [0.26, 0.26, 0.01])
                .color(*rgb)
                .material("H_Matte")
                .attr("albedo_srgb", format!("[{}, {}, {}]", rgb[0], rgb[1], rgb[2])),
        );
    }

    // Three fixed light sources at declared positions.
    s.folder("Workspace/Lights");
    s.lamp("Workspace/Lights/Key", [4.0, 5.0, 4.0], "PointLight", [255, 255, 255], 4.0, 30.0, "");
    s.lamp("Workspace/Lights/Fill", [-5.0, 3.0, 3.0], "PointLight", [255, 255, 255], 1.5, 30.0, "");
    s.lamp("Workspace/Lights/Rim", [0.0, 4.0, -6.0], "PointLight", [255, 255, 255], 2.5, 30.0, "");
}

// ─────────────────────────────────────────────────────────────────────────────
// S2: interior
// ─────────────────────────────────────────────────────────────────────────────

fn build_s2(s: &mut Scene) {
    common_materials(s);
    s.material("H_Metal", "Harness: polished metal", "metallic = 1.0\nroughness = 0.15\nreflectance = 0.5\n");
    s.material("H_Dielectric", "Harness: painted dielectric", "metallic = 0.0\nroughness = 0.45\nreflectance = 0.5\n");
    s.material(
        "H_ThinShell",
        "Harness: zero-thickness transmissive shell",
        "metallic = 0.0\nroughness = 0.05\nreflectance = 0.5\nalpha_mode = \"blend\"\ndouble_sided = true\nior = 1.5\nspecular_transmission = 0.9\nthickness = 0.0\n",
    );
    s.lighting = LightingSetup {
        ambient: [0.0, 0.0, 0.0],
        outdoor_ambient: [0.0, 0.0, 0.0],
        environment_diffuse_scale: 0.0,
        environment_specular_scale: 0.0,
        sun_enabled: false,
        ..LightingSetup::default()
    };

    // Room interior: x in [-3, 3] (6 m), z in [-4, 4] (8 m), y in [0, 3]; walls 0.2 m.
    let (w, d, h, t) = (6.0, 8.0, 3.0, 0.2);
    s.folder("Workspace/Room");
    let wall = [200, 196, 188];
    s.part(Part::block("Workspace/Room/Floor", [0.0, -t / 2.0, 0.0], [w + 2.0 * t, t, d + 2.0 * t]).color([150, 120, 90]).material("H_Matte"));
    s.part(Part::block("Workspace/Room/Ceiling", [0.0, h + t / 2.0, 0.0], [w + 2.0 * t, t, d + 2.0 * t]).color(wall).material("H_Matte"));
    s.part(Part::block("Workspace/Room/WallWest", [-w / 2.0 - t / 2.0, h / 2.0, 0.0], [t, h, d]).color(wall).material("H_Matte"));
    s.part(Part::block("Workspace/Room/WallNorth", [0.0, h / 2.0, -d / 2.0 - t / 2.0], [w, h, t]).color(wall).material("H_Matte"));
    s.part(Part::block("Workspace/Room/WallSouth", [0.0, h / 2.0, d / 2.0 + t / 2.0], [w, h, t]).color(wall).material("H_Matte"));
    // East wall with one window: 1.6 m wide (z in [-0.8, 0.8]), sill 1.0 m, head 2.2 m.
    let x = w / 2.0 + t / 2.0;
    let (wz0, wz1, sill, head) = (-0.8, 0.8, 1.0, 2.2);
    s.part(Part::block("Workspace/Room/WallEast_North", [x, h / 2.0, (-d / 2.0 + wz0) / 2.0], [t, h, wz0 + d / 2.0]).color(wall).material("H_Matte"));
    s.part(Part::block("Workspace/Room/WallEast_South", [x, h / 2.0, (wz1 + d / 2.0) / 2.0], [t, h, d / 2.0 - wz1]).color(wall).material("H_Matte"));
    s.part(Part::block("Workspace/Room/WallEast_Below", [x, sill / 2.0, 0.0], [t, sill, wz1 - wz0]).color(wall).material("H_Matte"));
    s.part(Part::block("Workspace/Room/WallEast_Above", [x, (head + h) / 2.0, 0.0], [t, h - head, wz1 - wz0]).color(wall).material("H_Matte"));
    s.note("window", "east wall, z -0.8..0.8 m, y 1.0..2.2 m, open aperture".to_string());

    // The directional source comes in through the window from the east, 30 degrees down.
    s.directional_light("WindowSun", [-0.866025, -0.5, 0.0], [255, 244, 229], 1.0);

    // Two artificial sources of differing colour temperature (CONFIG DEFAULT
    // sRGB approximations of 2700 K and 6500 K blackbodies).
    s.folder("Workspace/Lamps");
    s.lamp("Workspace/Lamps/Warm_2700K", [-1.8, 2.7, -2.5], "PointLight", [255, 169, 87], 2.0, 12.0, "");
    s.lamp("Workspace/Lamps/Cool_6500K", [-1.8, 2.7, 2.5], "PointLight", [255, 249, 253], 2.0, 12.0, "");
    s.note("lamp_colour_temperature_k", "2700 (Warm_2700K), 6500 (Cool_6500K)".to_string());

    // Objects spanning metal, dielectric, thin-shell and emissive, each
    // jittered within its cell by the seed.
    s.folder("Workspace/Objects");
    let j = |s: &mut Scene, base: [f64; 3]| [base[0] + s.rng.range(-0.3, 0.3), base[1], base[2] + s.rng.range(-0.3, 0.3)];
    let p = j(s, [-1.0, 0.4, -1.5]);
    s.part(Part::ball("Workspace/Objects/MetalSphere", p, 0.8).color([230, 230, 230]).material("H_Metal").attr("surface", q("metal")));
    let p = j(s, [1.0, 0.35, -1.5]);
    s.part(Part::block("Workspace/Objects/DielectricCube", p, [0.7, 0.7, 0.7]).color([180, 40, 40]).material("H_Dielectric").attr("surface", q("dielectric")));
    let p = j(s, [-1.0, 0.75, 1.5]);
    let mut pane = Part::block("Workspace/Objects/ThinShellPane", p, [1.0, 1.5, 0.002]).color([255, 255, 255]).material("H_ThinShell").attr("surface", q("thin_shell"));
    pane.transparency = 0.0;
    s.part(pane);
    let p = j(s, [1.0, 0.5, 1.5]);
    let mut panel = Part::block("Workspace/Objects/EmissivePanel", p, [0.6, 1.0, 0.05]).color([255, 230, 200]).material("H_Emissive").attr("surface", q("emissive"));
    panel.cast_shadow = false;
    s.part(panel);
}

// ─────────────────────────────────────────────────────────────────────────────
// S3: exterior
// ─────────────────────────────────────────────────────────────────────────────

/// CONFIG DEFAULT: the declared directional source.
const S3_SUN_ELEVATION_DEG: f64 = 35.0;
const S3_SUN_AZIMUTH_DEG: f64 = 150.0;
/// Occluder heights above the receiving ground, metres.
const S3_OCCLUDER_DISTANCES_M: [f64; 8] = [0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 20.0];

fn build_s3(s: &mut Scene) {
    common_materials(s);
    s.lighting = LightingSetup {
        ambient: [0.0, 0.0, 0.0],
        outdoor_ambient: [0.0, 0.0, 0.0],
        environment_diffuse_scale: 0.0,
        environment_specular_scale: 0.0,
        sun_enabled: false,
        atmosphere_environment: false,
        celestial_bodies_shown: false,
        ..LightingSetup::default()
    };
    s.note("sun", format!("one DirectionalLight, elevation {} deg, azimuth {} deg clockwise from north (-Z); the Lighting sun, moon and sky environment light are off", S3_SUN_ELEVATION_DEG, S3_SUN_AZIMUTH_DEG));
    s.directional_light("DeclaredSun", sun_travel_dir(S3_SUN_ELEVATION_DEG, S3_SUN_AZIMUTH_DEG), [255, 250, 242], 1.0);

    // Open ground: a 400 m square, matte grass-green, top at y = 0.
    s.part(Part::block("Workspace/Ground", [0.0, -0.5, 0.0], [400.0, 1.0, 400.0]).color([96, 120, 70]).material("H_Matte"));

    // Occluders: a 1 m x 0.1 m x 1 m slab held at each distance above the
    // ground, spaced along X, jittered in Z by the seed.
    s.folder("Workspace/Occluders");
    for (i, dist) in S3_OCCLUDER_DISTANCES_M.iter().enumerate() {
        let x = -28.0 + i as f64 * 8.0;
        let z = s.rng.range(-1.0, 1.0);
        let name = format!("Workspace/Occluders/Slab_{:02}_{}m", i, ff(*dist).replace('.', "p"));
        s.part(
            Part::block(name, [x, dist + 0.05, z], [1.0, 0.1, 1.0])
                .color([60, 60, 60])
                .material("H_Matte")
                .attr("occluder_distance_m", ff(*dist)),
        );
    }
    // A post at the origin gives a contact shadow (distance 0) for reference.
    s.part(Part::block("Workspace/Occluders/ContactPost", [0.0, 1.0, 6.0], [0.2, 2.0, 0.2]).color([60, 60, 60]).material("H_Matte").attr("occluder_distance_m", ff(0.0)));
}

// ─────────────────────────────────────────────────────────────────────────────
// S4: studio UI
// ─────────────────────────────────────────────────────────────────────────────

fn build_s4(s: &mut Scene) {
    common_materials(s);
    s.part(Part::block("Workspace/Baseplate", [0.0, -0.5, 0.0], [64.0, 1.0, 64.0]).color([99, 95, 98]).material("SmoothPlastic"));
    s.folder("Workspace/Props");
    let colors: [[u8; 3]; 4] = [[196, 40, 28], [13, 105, 172], [75, 151, 75], [245, 205, 48]];
    for (i, c) in colors.iter().enumerate() {
        let x = -4.5 + 3.0 * i as f64;
        let z = s.rng.range(-0.5, 0.5);
        s.part(Part::block(format!("Workspace/Props/Prop_{i}"), [x, 1.0, z], [2.0, 2.0, 2.0]).color(*c).material("SmoothPlastic"));
    }
    let selected = s.part(Part::block("Workspace/Props/PinnedSelection", [0.0, 0.5, -4.0], [4.0, 1.0, 2.0]).color([163, 162, 165]).material("SmoothPlastic"));

    // Pins the studio is to restore: selection, panel layout, window size.
    // The layout file uses the studio's `PanelLayout` JSON shape
    // (crates/engine/src/ui/layout_tables.rs), the same shape the studio's
    // Save Layout writes and Import Layout reads.
    s.put(
        ".eustress/harness/layout.json",
        "{\n  \"show_explorer\": true,\n  \"show_properties\": true,\n  \"show_output\": true,\n  \"show_toolbox\": false,\n  \"show_assets\": false,\n  \"show_history\": false,\n  \"show_soul_panel\": false,\n  \"left_panel_width\": 280.0,\n  \"right_panel_width\": 320.0,\n  \"output_height\": 180.0,\n  \"left_tab_index\": 0,\n  \"right_tab_index\": 0,\n  \"preset\": 0\n}\n".to_string(),
    );
    s.put(
        ".eustress/harness/studio_pins.toml",
        format!(
            "# S4 studio pins. Restoring these on load is the studio's job; see the G1.03 report.\n[selection]\npath = \"Workspace/Props/PinnedSelection\"\nuuid = {}\n\n[layout]\nfile = \"layout.json\"\n\n[window]\nwidth = 1920\nheight = 1080\n\n[camera]\nposition = [8.0, 6.0, 8.0]\nlook_at = [0.0, 0.5, -1.0]\n",
            q(&selected)
        ),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// S5: physics stress
// ─────────────────────────────────────────────────────────────────────────────

/// CONFIG DEFAULT: one density for every S5 body, so mass ratios come from
/// volume alone (1 m cube / 0.1 m cube = 1000:1).
const S5_DENSITY: f64 = 1000.0;

fn build_s5(s: &mut Scene) {
    common_materials(s);
    s.note("physics_config", "none: S5 runs on the engine's pinned 60 Hz fixed step and SubstepCount(6); PhysicsService is the shipped template".to_string());
    s.note("rest_probe", "Workspace/Rest/RestingCube, hold 600 ticks, read position jitter".to_string());
    s.simulation_toml = Some(simulation_toml(false, false, ""));

    let mut ground = Part::block("Workspace/Ground", [0.0, -0.5, 0.0], [40.0, 1.0, 40.0]).color([110, 110, 110]).material("Concrete");
    ground.friction = Some(0.6);
    s.part(ground);
    let body = |p: Part| {
        let mut p = p.dynamic();
        p.density = Some(S5_DENSITY);
        p.friction = Some(0.6);
        p
    };

    // Mass ratio 1000:1, both ways round.
    s.folder("Workspace/MassRatio");
    s.part(body(Part::block("Workspace/MassRatio/Heavy_1000kg", [0.0, 0.5, 0.0], [1.0, 1.0, 1.0])).color([90, 90, 160]).attr("mass_kg", ff(1000.0)));
    s.part(body(Part::block("Workspace/MassRatio/LightOnHeavy_1kg", [0.0, 1.05, 0.0], [0.1, 0.1, 0.1])).color([200, 200, 80]).attr("mass_kg", ff(1.0)));
    for (i, (dx, dz)) in [(-0.4, -0.4), (0.4, -0.4), (-0.4, 0.4), (0.4, 0.4)].iter().enumerate() {
        s.part(body(Part::block(format!("Workspace/MassRatio/Leg_{i}_1kg"), [4.0 + dx, 0.05, *dz], [0.1, 0.1, 0.1])).color([200, 200, 80]).attr("mass_kg", ff(1.0)));
    }
    s.part(body(Part::block("Workspace/MassRatio/HeavyOnLegs_1000kg", [4.0, 0.6, 0.0], [1.0, 1.0, 1.0])).color([90, 90, 160]).attr("mass_kg", ff(1000.0)));

    // A stack of eight 0.5 m cubes settling under gravity, placed in exact contact.
    s.folder("Workspace/Stack");
    for k in 0..8 {
        s.part(body(Part::block(format!("Workspace/Stack/Cube_{k}"), [-4.0, 0.25 + 0.5 * k as f64, 0.0], [0.5, 0.5, 0.5])).color([160, 100, 60]));
    }

    // A joint chain: eight 0.4 m links held horizontally from an anchored
    // post, joined end to end by BallSocketConstraints between Attachments.
    s.folder("Workspace/Chain");
    let anchor_pos = [-2.0, 6.0, -5.0];
    let anchor = "Workspace/Chain/Anchor";
    s.part(Part::block(anchor, anchor_pos, [0.2, 0.2, 0.2]).color([40, 40, 40]));
    let mut prev_attachment = attachment(s, anchor, "A1", [0.1, 0.0, 0.0]);
    for k in 0..8 {
        let link = format!("Workspace/Chain/Link_{k}");
        let x = anchor_pos[0] + 0.1 + 0.2 + 0.4 * k as f64;
        s.part(body(Part::block(link.as_str(), [x, anchor_pos[1], anchor_pos[2]], [0.4, 0.08, 0.08])).color([180, 180, 190]));
        let a0 = attachment(s, &link, "A0", [-0.2, 0.0, 0.0]);
        let a1 = attachment(s, &link, "A1", [0.2, 0.0, 0.0]);
        let joint = format!("Workspace/Chain/Joint_{k}");
        let text = format!(
            "[metadata]\nclass_name = \"BallSocketConstraint\"\narchivable = true\nname = {}\nuuid = {}\n\n[constraint]\nenabled = true\nlimits_enabled = false\nupper_angle = 45.0\n\n[references]\nAttachment0 = {}\nAttachment1 = {}\n",
            q(&format!("Joint_{k}")),
            q(&s.uuid(&joint)),
            q(&prev_attachment),
            q(&a0)
        );
        s.put(&format!("{joint}/_instance.toml"), text);
        prev_attachment = a1;
    }

    // High-restitution impact: a ball dropped from 3 m.
    s.folder("Workspace/Impact");
    let mut plate = Part::block("Workspace/Impact/Plate", [6.0, 0.05, -5.0], [2.0, 0.1, 2.0]).color([70, 70, 70]).material("Metal");
    plate.restitution = Some(0.95);
    s.part(plate);
    let drop_x = 6.0 + s.rng.range(-0.05, 0.05);
    let mut ball = body(Part::ball("Workspace/Impact/Ball", [drop_x, 3.0, -5.0], 0.3)).color([220, 60, 60]);
    ball.restitution = Some(0.95);
    s.part(ball.attr("drop_height_m", ff(3.0)));

    // A body resting in exact contact, held for 600 ticks to expose jitter.
    s.folder("Workspace/Rest");
    s.part(body(Part::block("Workspace/Rest/RestingCube", [-6.0, 0.25, 5.0], [0.5, 0.5, 0.5])).color([60, 160, 60]).attr("hold_ticks", "600".to_string()));
}

/// An Attachment child of `parent` at a part-local offset; returns its uuid.
fn attachment(s: &mut Scene, parent: &str, name: &str, local: [f64; 3]) -> String {
    let path = format!("{parent}/{name}");
    let uuid = s.uuid(&path);
    let text = format!(
        "[metadata]\nclass_name = \"Attachment\"\narchivable = true\nname = {}\nuuid = {}\n\n[attachment]\nvisible = false\naxis = [1.0, 0.0, 0.0]\nsecondary_axis = [0.0, 1.0, 0.0]\n\n[transform]\nposition = {}\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [0.1, 0.1, 0.1]\n\n[properties]\nanchored = true\ncan_collide = false\ncast_shadow = false\ntransparency = 1.0\n",
        q(name),
        q(&uuid),
        v3(local)
    );
    s.put(&format!("{path}.instance.toml"), text);
    uuid
}

// ─────────────────────────────────────────────────────────────────────────────
// S6: domain simulation
// ─────────────────────────────────────────────────────────────────────────────

/// CONFIG DEFAULT: copper rod, 16 nodes, 0.02 m apart.
const S6_NODES: usize = 16;
const S6_DX_M: f64 = 0.02;
/// Copper: k = 401 W/(m K), rho = 8960 kg/m3, c = 385 J/(kg K).
const S6_ALPHA_M2_S: f64 = 401.0 / (8960.0 * 385.0);
/// Simulated seconds per step (time compression); r = alpha dt / dx^2 < 0.5.
const S6_DT_S: f64 = 1.0;
const S6_HOT_K: f64 = 373.15;
const S6_AMBIENT_K: f64 = 293.15;
/// Lumped convective loss at the cold end, per step.
const S6_END_LOSS: f64 = 0.01;

fn build_s6(s: &mut Scene) {
    common_materials(s);
    let r = S6_ALPHA_M2_S * S6_DT_S / (S6_DX_M * S6_DX_M);
    s.note("model", format!(
        "explicit FTCS heat conduction in a {} m copper rod, {} nodes, dx {} m, alpha {} m2/s, dt {} s per step, r {}; node 0 held at {} K, last node loses heat to {} K",
        ff(S6_DX_M * (S6_NODES - 1) as f64), S6_NODES, ff(S6_DX_M), ff(S6_ALPHA_M2_S * 1.0e6) + "e-6", ff(S6_DT_S), ff(r), ff(S6_HOT_K), ff(S6_AMBIENT_K)
    ));
    s.note("step_counter", "rod_step_count, advanced once per on_update by the script, independent of the engine tick".to_string());

    s.part(Part::block("Workspace/Bench", [0.0, -0.5, 0.0], [4.0, 1.0, 4.0]).color([120, 110, 100]).material("H_Matte"));
    s.folder("Workspace/Rod");
    for i in 0..S6_NODES {
        let x = -0.15 + S6_DX_M * i as f64;
        let mut seg = Part::block(format!("Workspace/Rod/Node_{i:02}"), [x, 0.05, 0.0], [S6_DX_M, 0.02, 0.02]).color([184, 115, 51]).material("Metal");
        seg.attributes.push(("node_index".to_string(), i.to_string()));
        s.part(seg);
    }

    // Initial temperatures: ambient plus a seeded perturbation under 0.05 K.
    let init: Vec<f64> = (0..S6_NODES).map(|_| S6_AMBIENT_K + s.rng.range(-0.05, 0.05)).collect();

    let key = |i: usize| format!("rod_T{i:02}");
    let mut src = String::new();
    src.push_str("// Copper rod heat conduction, emitted by harness-scenes (G1.03). Never edit by hand.\n");
    src.push_str("// State lives in sim values so it persists between frames; every frame advances one step.\n\n");
    src.push_str("pub fn on_init() {\n    if get_sim_value(\"rod_initialised\") < 0.5 {\n");
    for (i, t) in init.iter().enumerate() {
        let t0 = if i == 0 { S6_HOT_K } else { *t };
        src.push_str(&format!("        set_sim_value(\"{}\", {});\n", key(i), ff(t0)));
    }
    src.push_str("        set_sim_value(\"rod_step_count\", 0.0);\n        set_sim_value(\"rod_sim_time_s\", 0.0);\n        set_sim_value(\"rod_initialised\", 1.0);\n    }\n}\n\n");
    src.push_str("pub fn on_update(_dt) {\n");
    src.push_str(&format!("    let r = {};\n", ff(r)));
    for i in 0..S6_NODES {
        src.push_str(&format!("    let t{i} = get_sim_value(\"{}\");\n", key(i)));
    }
    src.push_str(&format!("    let n0 = {};\n", ff(S6_HOT_K)));
    for i in 1..S6_NODES - 1 {
        src.push_str(&format!("    let n{i} = t{i} + r * (t{} - 2.0 * t{i} + t{});\n", i - 1, i + 1));
    }
    let last = S6_NODES - 1;
    src.push_str(&format!(
        "    let n{last} = t{last} + r * (t{} - t{last}) - {} * (t{last} - {});\n",
        last - 1,
        ff(S6_END_LOSS),
        ff(S6_AMBIENT_K)
    ));
    for i in 0..S6_NODES {
        src.push_str(&format!("    set_sim_value(\"{}\", n{i});\n", key(i)));
    }
    let sum: Vec<String> = (0..S6_NODES).map(|i| format!("n{i}")).collect();
    src.push_str(&format!("    set_sim_value(\"rod_mean_T\", ({}) / {});\n", sum.join(" + "), ff(S6_NODES as f64)));
    src.push_str("    let steps = get_sim_value(\"rod_step_count\") + 1.0;\n    set_sim_value(\"rod_step_count\", steps);\n");
    src.push_str(&format!("    set_sim_value(\"rod_sim_time_s\", steps * {});\n}}\n", ff(S6_DT_S)));
    s.put("ServerScriptService/RodHeatModel/RodHeatModel.rune", src);
    let script = "ServerScriptService/RodHeatModel";
    let text = format!(
        "[metadata]\nclass_name = \"SoulScript\"\narchivable = true\nname = \"RodHeatModel\"\nuuid = {}\n\n[script]\nsource = \"RodHeatModel.rune\"\nenabled = true\nrun_context = \"Rune\"\n",
        q(&s.uuid(script))
    );
    s.put(&format!("{script}/_instance.toml"), text);

    let mut wp = String::new();
    for (name, label, unit) in [
        ("rod_T00", "Hot end temperature", "K"),
        ("rod_T05", "Node 5 temperature", "K"),
        ("rod_T10", "Node 10 temperature", "K"),
        ("rod_T15", "Cold end temperature", "K"),
        ("rod_mean_T", "Mean rod temperature", "K"),
        ("rod_step_count", "Independent step counter", "steps"),
        ("rod_sim_time_s", "Simulated time", "s"),
    ] {
        wp.push_str(&format!("\n[[watchpoints]]\nname = \"{name}\"\nlabel = \"{label}\"\nunit = \"{unit}\"\ninterval = 1\n"));
    }
    s.simulation_toml = Some(simulation_toml(true, true, &wp));
}

// ─────────────────────────────────────────────────────────────────────────────
// capture-selftest (G1.05): drive the running engine's AI camera
// ─────────────────────────────────────────────────────────────────────────────

fn capture_selftest(args: &[String]) -> i32 {
    let Some(universe) = value(args, "--universe").map(PathBuf::from) else {
        eprintln!("ERROR: --universe <dir> is required");
        return 2;
    };
    let parse3 = |name: &str| -> Option<[f64; 3]> {
        let v = values(args, name, 3)?;
        Some([v[0].parse().ok()?, v[1].parse().ok()?, v[2].parse().ok()?])
    };
    let (Some(pos), Some(look)) = (parse3("--pose-position"), parse3("--pose-look-at")) else {
        eprintln!("ERROR: --pose-position X Y Z and --pose-look-at X Y Z are required");
        return 2;
    };
    let Some(sizes_arg) = value(args, "--sizes") else {
        eprintln!("ERROR: --sizes WxH,WxH,... is required");
        return 2;
    };
    let mut sizes = Vec::new();
    for item in sizes_arg.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let parsed = item.split_once(['x', 'X']).and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)));
        match parsed {
            Some(wh) => sizes.push(wh),
            None => {
                eprintln!("ERROR: bad size {item:?}; expected WxH");
                return 2;
            }
        }
    }
    let Some(out) = value(args, "--out").map(PathBuf::from) else {
        eprintln!("ERROR: --out <json> is required");
        return 2;
    };
    let out = absolute(&out);
    let capture_dir = out.parent().map(|p| p.join("captures")).unwrap_or_else(|| PathBuf::from("captures"));
    let _ = std::fs::create_dir_all(&capture_dir);
    let universe = absolute(&universe);
    let port_file = eustress_bridge_client::port_file_path(&universe);
    let port = std::fs::read_to_string(&port_file).ok().map(|s| s.trim().to_string());

    let mut errors: Vec<String> = Vec::new();
    let pose = eustress_bridge_client::call_engine_with_timeout(
        &universe,
        "ai_camera.set_pose",
        serde_json::json!({ "position": pos, "look_at": look }),
        Duration::from_secs(10),
    );
    if let Err(e) = &pose {
        errors.push(format!("set_pose: {e}"));
    }

    let mut rows = Vec::new();
    let mut mismatches = 0usize;
    let mut four_k: Vec<String> = Vec::new();
    if pose.is_ok() {
        for (i, (w, h)) in sizes.iter().enumerate() {
            let n = i + 1;
            let path = capture_dir.join(format!("capture_{n:02}_{w}x{h}.png"));
            let _ = std::fs::remove_file(&path);
            let reply = eustress_bridge_client::call_engine_with_timeout(
                &universe,
                "ai_camera.capture",
                serde_json::json!({ "width": w, "height": h, "path": path.to_string_lossy() }),
                Duration::from_secs(30),
            );
            let reply = match reply {
                Ok(v) => v,
                Err(e) => {
                    errors.push(format!("capture {n}: {e}"));
                    mismatches += 1;
                    rows.push(serde_json::json!({ "index": n, "requested": { "width": w, "height": h }, "error": e }));
                    continue;
                }
            };
            let landed = reply.get("path").and_then(|v| v.as_str()).map(PathBuf::from).unwrap_or_else(|| path.clone());
            match wait_for_stable_file(&landed, Duration::from_secs(120)).and_then(|bytes| {
                let (ew, eh) = png_ihdr_size(&bytes)?;
                Ok((bytes, ew, eh))
            }) {
                Ok((bytes, ew, eh)) => {
                    let sha = hex(&Sha256::digest(&bytes));
                    let matched = ew == *w && eh == *h;
                    if !matched {
                        mismatches += 1;
                    }
                    if *w == 3840 && *h == 2160 {
                        four_k.push(sha.clone());
                    }
                    println!("{n} requested={w}x{h} emitted={ew}x{eh} sha256={sha} path={}", landed.display());
                    rows.push(serde_json::json!({
                        "index": n,
                        "requested": { "width": w, "height": h },
                        "emitted": { "width": ew, "height": eh },
                        "dimensions_match": matched,
                        "sha256": sha,
                        "bytes": bytes.len(),
                        "path": landed.to_string_lossy(),
                        "bridge_reply": reply,
                    }));
                }
                Err(e) => {
                    mismatches += 1;
                    errors.push(format!("capture {n}: {e}"));
                    println!("{n} requested={w}x{h} emitted=none sha256=none path={}", landed.display());
                    rows.push(serde_json::json!({ "index": n, "requested": { "width": w, "height": h }, "error": e, "path": landed.to_string_lossy() }));
                }
            }
        }
    }
    let repeat_identical = four_k.len() >= 2 && four_k.iter().all(|s| *s == four_k[0]);
    let repeat_failed = four_k.len() >= 2 && !repeat_identical;
    println!("captures={} dimension_mismatches={} repeat_4k_identical={}", rows.len(), mismatches, repeat_identical);
    for e in &errors {
        eprintln!("error: {e}");
    }

    let report = serde_json::json!({
        "item": "G1.05",
        "tool": format!("{BIN_NAME} capture-selftest"),
        "universe": universe.to_string_lossy(),
        "engine_port_file": port_file.to_string_lossy(),
        "engine_port": port,
        "pose": { "position": pos, "look_at": look },
        "captures": rows,
        "summary": {
            "captures": sizes.len(),
            "dimension_mismatches": mismatches,
            "repeat_4k_count": four_k.len(),
            "repeat_4k_identical": repeat_identical,
        },
        "errors": errors,
    });
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&out, serde_json::to_string_pretty(&report).unwrap_or_default() + "\n") {
        eprintln!("ERROR: cannot write {}: {e}", out.display());
        return 5;
    }
    if pose.is_err() || mismatches > 0 || repeat_failed || rows.len() != sizes.len() {
        1
    } else {
        0
    }
}

/// Wait until `path` exists and its size holds still for three polls.
fn wait_for_stable_file(path: &Path, timeout: Duration) -> Result<Vec<u8>, String> {
    let start = Instant::now();
    let mut last: Option<u64> = None;
    let mut stable = 0;
    while start.elapsed() < timeout {
        if let Ok(meta) = std::fs::metadata(path) {
            let len = meta.len();
            if Some(len) == last && len >= 24 {
                stable += 1;
                if stable >= 3 {
                    return std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()));
                }
            } else {
                stable = 0;
            }
            last = Some(len);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!("{} did not appear and settle within {} s", path.display(), timeout.as_secs()))
}

/// Width and height from the IHDR chunk in a PNG's first 24 bytes.
fn png_ihdr_size(bytes: &[u8]) -> Result<(u32, u32), String> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 24 || bytes[..8] != SIG || &bytes[12..16] != b"IHDR" {
        return Err("not a PNG with a leading IHDR chunk".to_string());
    }
    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    Ok((w, h))
}

// ─────────────────────────────────────────────────────────────────────────────
// frametime (G1.08): run the windowed engine with the frame-time recorder armed
// ─────────────────────────────────────────────────────────────────────────────

/// CONFIG DEFAULT: how long the engine may run before it is stopped.
const FRAMETIME_TIMEOUT_S: u64 = 900;

fn frametime(args: &[String]) -> i32 {
    let (Some(space), Some(path_name), Some(csv), Some(summary)) = (
        value(args, "--space").map(PathBuf::from),
        value(args, "--path"),
        value(args, "--csv").map(PathBuf::from),
        value(args, "--summary").map(PathBuf::from),
    ) else {
        eprintln!("ERROR: --space, --path, --csv and --summary are required");
        return 2;
    };
    let (Some(width), Some(height)) = (
        value(args, "--width").and_then(|v| v.parse::<u32>().ok()),
        value(args, "--height").and_then(|v| v.parse::<u32>().ok()),
    ) else {
        eprintln!("ERROR: --width and --height must be whole numbers of pixels");
        return 2;
    };
    let timeout = value(args, "--timeout-s").and_then(|v| v.parse::<u64>().ok()).unwrap_or(FRAMETIME_TIMEOUT_S);
    let frames: u64 = std::env::var("EUSTRESS_PROFILE_FRAMES").ok().and_then(|v| v.parse().ok()).filter(|n| *n > 0).unwrap_or(600);
    let space = absolute(&space);
    let csv = absolute(&csv);
    let summary = absolute(&summary);
    if !space.join("space.toml").is_file() {
        eprintln!("ERROR: {} is not a Space (no space.toml)", space.display());
        return 2;
    }

    // The scene id and seed from the harness marker, when there is one.
    let marker: Option<toml::Value> = std::fs::read_to_string(space.join(MARKER)).ok().and_then(|t| t.parse().ok());
    let scene_id = marker.as_ref().and_then(|m| m.get("harness")?.get("scene_id")?.as_str().map(str::to_string));
    let seed = marker.as_ref().and_then(|m| m.get("harness")?.get("seed")?.as_integer());
    let space_sha = tree_digest(&space).map(|d| d.sha256).ok();

    // A frozen scene is never opened in place (the engine writes its runtime
    // state into an open Space); run a byte-identical copy unless told not to.
    let run_space = if flag(args, "--in-place") {
        space.clone()
    } else {
        let name = space.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Space".to_string());
        let copy = std::env::temp_dir().join("eustress-harness-run").join("Spaces").join(&name);
        let _ = std::fs::remove_dir_all(&copy);
        let files = match tree_files(&space) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("ERROR: cannot read {}: {e}", space.display());
                return 3;
            }
        };
        if let Err(e) = write_scene_dir(&copy, &files, true) {
            eprintln!("ERROR: cannot copy the Space: {e}");
            return 3;
        }
        if tree_digest(&copy).map(|d| d.sha256).ok() != space_sha {
            eprintln!("ERROR: the copy of {} does not hash like the original", space.display());
            return 3;
        }
        copy
    };

    let exe_name = format!("eustress-engine{}", std::env::consts::EXE_SUFFIX);
    let Some(engine) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(&exe_name))) else {
        eprintln!("ERROR: cannot locate the current executable");
        return 3;
    };
    if !engine.is_file() {
        eprintln!("ERROR: {} not found; build it first (cargo build --release --bin eustress-engine)", engine.display());
        return 3;
    }
    for p in [&csv, &summary] {
        let _ = std::fs::remove_file(p);
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let context = serde_json::json!({
        "scene_id": scene_id,
        "scene": space.file_name().map(|n| n.to_string_lossy().to_string()),
        "scene_sha256": space_sha,
        "camera_path": path_name,
        "seed": seed,
        "resolution": format!("{width}x{height}"),
        "width": width,
        "height": height,
        "frames": frames,
        "ran_from_copy": run_space != space,
        "driver": format!("{BIN_NAME} frametime"),
    });

    println!("frametime: {} --space {}", engine.display(), run_space.display());
    let mut child = match std::process::Command::new(&engine)
        .arg("--space")
        .arg(&run_space)
        .env("EUSTRESS_FRAMETIME_CSV", &csv)
        .env("EUSTRESS_FRAMETIME_SUMMARY", &summary)
        .env("EUSTRESS_FRAMETIME_FRAMES", frames.to_string())
        .env("EUSTRESS_FRAMETIME_EXIT", "1")
        .env("EUSTRESS_FRAMETIME_CONTEXT", context.to_string())
        .env("EUSTRESS_CAMERA_PATH", &path_name)
        .env("EUSTRESS_CAMERA_PATH_FILE", space.parent().map(|p| p.join("paths").join(format!("{path_name}.json"))).unwrap_or_default())
        .env("EUSTRESS_WINDOW_SIZE", format!("{width}x{height}"))
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ERROR: cannot start {}: {e}", engine.display());
            return 3;
        }
    };
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() > Duration::from_secs(timeout) => {
                eprintln!("ERROR: the engine (pid {}) ran past {timeout} s; stopping it", child.id());
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(500)),
            Err(e) => {
                eprintln!("ERROR: waiting on the engine: {e}");
                break None;
            }
        }
    };

    let rows = std::fs::read_to_string(&csv).ok().map(|t| t.lines().skip(1).filter(|l| !l.trim().is_empty()).count());
    let p99 = std::fs::read_to_string(&summary)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("ft_p99_ms").and_then(|x| x.as_f64()));
    println!(
        "engine_exit={} csv_rows={} ft_p99_ms={} csv={} summary={}",
        status.map(|s| s.to_string()).unwrap_or_else(|| "none".to_string()),
        rows.map(|r| r.to_string()).unwrap_or_else(|| "missing".to_string()),
        p99.map(|p| p.to_string()).unwrap_or_else(|| "missing".to_string()),
        csv.display(),
        summary.display()
    );
    let _ = std::io::stdout().flush();
    if rows == Some(frames as usize) && p99.is_some() {
        0
    } else {
        1
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// paths (G1.04): the camera path files CP-A, CP-B, CP-S, CP-U and their replay check
//
// Everything below belongs to the `paths` subcommand. It is reachable only from
// `main`'s `paths` arm, never from the generator path, and it uses no engine
// library code. The poses are CONFIG DEFAULT values of this generator against
// scene S3 (`RH3_exterior`); nothing here is MEASURED until `paths --verify` runs.
// ─────────────────────────────────────────────────────────────────────────────

const PATH_SCHEMA: &str = "eustress.camera_path/1";
const PATH_LOG_SCHEMA: &str = "eustress.camera_path_log/1";
const PATH_RATE_HZ: u32 = 60;
/// Declared by `02_CAPTURE_HARNESS.md` 4.1: CP-A is ticks 0-179, CP-B ticks 0-599.
const CPA_TICKS: usize = 180;
const CPB_TICKS: usize = 600;
/// FS-MATERIAL names 24 enumerated poses (`02` 4.1).
const CPS_TICKS: usize = 24;
const PATH_SCENE_ID: &str = "S3";
const PATH_SCENE_DIR: &str = "RH3_exterior";
const PATH_ENGINE_TIMEOUT_S: u64 = 300;
/// CONFIG DEFAULT: the point the orbit and the dolly frame (metres, scene S3).
const PATH_HERO_M: [f64; 3] = [0.0, 3.0, 0.0];
/// CONFIG DEFAULT: CP-A starts here and dollies to `CPA_END_M` along a smoothstep.
const CPA_START_M: [f64; 3] = [0.0, 7.0, 46.0];
const CPA_END_M: [f64; 3] = [0.0, 4.5, 20.0];
/// CONFIG DEFAULT: CP-B orbit radius (metres) and camera elevation above the horizon.
const CPB_RADIUS_M: f64 = 36.0;
const CPB_PITCH_DEG: f64 = 20.0;
/// The path files this subcommand owns, in the order they are listed.
const PATH_IDS: [&str; 4] = ["CP-A", "CP-B", "CP-S", "CP-U"];
/// CP-U capture sizes (`02` 4.3).
const UI_SIZES: [(u32, u32); 3] = [(1920, 1080), (2560, 1440), (3840, 2160)];
/// CP-U sequences (`02` 4.3): name, then (action, target) per step. The fifth
/// step of `property_edit` is the re-read after a reload; it is the point of
/// that sequence and must not be dropped.
const UI_SEQUENCES: &[(&str, &[(&str, &str)])] = &[
    (
        "panel_open",
        &[
            ("idle", "studio at the pinned layout"),
            ("hover_panel_tab", "Properties panel tab"),
            ("click_panel_tab", "Properties panel tab"),
            ("panel_settled", "Properties panel"),
        ],
    ),
    (
        "entity_select",
        &[
            ("idle", "studio at the pinned layout"),
            ("hover_entity", "Workspace/Props/PinnedSelection"),
            ("click_entity", "Workspace/Props/PinnedSelection"),
            ("selection_settled_with_gizmo", "Workspace/Props/PinnedSelection"),
        ],
    ),
    (
        "property_edit",
        &[
            ("select_entity", "Workspace/Props/PinnedSelection"),
            ("focus_field", "Properties panel, a numeric field"),
            ("type_value", "Properties panel, a numeric field"),
            ("commit", "Properties panel, a numeric field"),
            ("reread_after_reload", "Properties panel, the same field after the Space is reloaded"),
        ],
    ),
    (
        "error_surface",
        &[
            ("trigger_known_invalid_action", "a known-invalid action"),
            ("error_visible", "the error surface"),
            ("error_dismissed", "the error surface"),
        ],
    ),
];

/// One enumerated pose: position in metres, rotation `[x, y, z, w]`.
#[derive(Clone, Copy, PartialEq)]
struct PathPose {
    position: [f32; 3],
    rotation: [f32; 4],
}

impl PathPose {
    fn bits(&self) -> [u32; 7] {
        [
            self.position[0].to_bits(),
            self.position[1].to_bits(),
            self.position[2].to_bits(),
            self.rotation[0].to_bits(),
            self.rotation[1].to_bits(),
            self.rotation[2].to_bits(),
            self.rotation[3].to_bits(),
        ]
    }
}

/// A number as the path file carries it: rounded to 6 decimals, then to `f32`,
/// with -0.0 folded to 0.0 so its text has one form.
fn path_f32(x: f64) -> f32 {
    let v = r6(x) as f32;
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

/// A camera at `pos` looking at `target`, +Y up.
fn pose_looking(pos: [f64; 3], target: [f64; 3]) -> PathPose {
    let q = look_rotation([target[0] - pos[0], target[1] - pos[1], target[2] - pos[2]]);
    PathPose {
        position: [path_f32(pos[0]), path_f32(pos[1]), path_f32(pos[2])],
        rotation: [path_f32(q[0]), path_f32(q[1]), path_f32(q[2]), path_f32(q[3])],
    }
}

/// CP-A: 180 poses along a smoothstep dolly toward the hero framing.
fn cpa_poses() -> Vec<(String, PathPose)> {
    (0..CPA_TICKS)
        .map(|i| {
            let t = i as f64 / (CPA_TICKS - 1) as f64;
            let s = t * t * (3.0 - 2.0 * t);
            let pos = [
                CPA_START_M[0] + (CPA_END_M[0] - CPA_START_M[0]) * s,
                CPA_START_M[1] + (CPA_END_M[1] - CPA_START_M[1]) * s,
                CPA_START_M[2] + (CPA_END_M[2] - CPA_START_M[2]) * s,
            ];
            (String::new(), pose_looking(pos, PATH_HERO_M))
        })
        .collect()
}

/// CP-B: 600 poses around a full circle (yaw 0 is +Z; the last pose is one
/// step short of the first, so the loop does not repeat a pose).
fn cpb_poses() -> Vec<(String, PathPose)> {
    let pitch = CPB_PITCH_DEG.to_radians();
    (0..CPB_TICKS)
        .map(|i| {
            let yaw = std::f64::consts::TAU * i as f64 / CPB_TICKS as f64;
            let pos = [
                PATH_HERO_M[0] + CPB_RADIUS_M * pitch.cos() * yaw.sin(),
                PATH_HERO_M[1] + CPB_RADIUS_M * pitch.sin(),
                PATH_HERO_M[2] + CPB_RADIUS_M * pitch.cos() * yaw.cos(),
            ];
            (String::new(), pose_looking(pos, PATH_HERO_M))
        })
        .collect()
}

/// Slab centres of scene S3, replayed from the scene's own random stream: the
/// stream is seeded by (seed, "S3"), `build_s3` first draws from it in the slab
/// loop, and each slab takes one `range(-1, 1)` for its Z.
fn s3_slab_centres(seed: u64) -> Vec<[f64; 3]> {
    let mut rng = Rng::new(seed, PATH_SCENE_ID);
    let mut out = Vec::new();
    for (i, dist) in S3_OCCLUDER_DISTANCES_M.iter().enumerate() {
        let x = -28.0 + i as f64 * 8.0;
        let z = rng.range(-1.0, 1.0);
        out.push([x, dist + 0.05, r6(z)]);
    }
    out
}

/// CP-S: 24 enumerated static studies, each at its own tick index.
fn cps_poses(seed: u64) -> Vec<(String, PathPose)> {
    let slabs = s3_slab_centres(seed);
    let (el, az) = (S3_SUN_ELEVATION_DEG.to_radians(), S3_SUN_AZIMUTH_DEG.to_radians());
    // Horizontal direction from the sun's source toward its shadow, per metre of height.
    let shadow_dir = [-az.sin() / el.tan(), az.cos() / el.tan()];
    let mut out: Vec<(String, PathPose)> = Vec::new();
    // 8 shadow-edge studies: where each slab's shadow lands on the ground.
    for (i, c) in slabs.iter().enumerate() {
        let h = S3_OCCLUDER_DISTANCES_M[i];
        let land = [c[0] + shadow_dir[0] * h, 0.0, c[2] + shadow_dir[1] * h];
        let pos = [land[0] + 2.0, land[1] + 1.8, land[2] + 3.5];
        out.push((format!("shadow_edge_{:02}_{}m", i, ff(h).replace('.', "p")), pose_looking(pos, land)));
    }
    // 8 slab studies: each slab seen side-on from 2.5 m, sun behind the camera.
    for (i, c) in slabs.iter().enumerate() {
        let pos = [c[0], c[1] + 0.4, c[2] + 2.5];
        out.push((format!("slab_side_{:02}_{}m", i, ff(S3_OCCLUDER_DISTANCES_M[i]).replace('.', "p")), pose_looking(pos, *c)));
    }
    // 2 contact-shadow studies of the post at the origin side.
    out.push(("post_contact_shadow_a".to_string(), pose_looking([3.0, 1.5, 8.5], [-0.7, 0.0, 4.8])));
    out.push(("post_contact_shadow_b".to_string(), pose_looking([-4.0, 3.0, 9.0], [-0.7, 0.0, 4.8])));
    // 6 whole-scene lighting studies.
    out.push(("wide_sun_behind_camera".to_string(), pose_looking([0.0, 2.0, 50.0], [0.0, 2.0, 0.0])));
    out.push(("wide_sun_facing".to_string(), pose_looking([0.0, 3.0, -50.0], [0.0, 2.0, 0.0])));
    out.push(("ground_grazing".to_string(), pose_looking([10.0, 0.5, 22.0], [0.0, 0.2, 0.0])));
    out.push(("ground_oblique_down".to_string(), pose_looking([0.0, 30.0, 6.0], [0.0, 0.0, 0.0])));
    out.push(("side_east".to_string(), pose_looking([50.0, 6.0, 0.0], PATH_HERO_M)));
    out.push(("side_west".to_string(), pose_looking([-50.0, 6.0, 0.0], PATH_HERO_M)));
    out
}

fn path_name(id: &str) -> &'static str {
    match id {
        "CP-A" => "first_impression",
        "CP-B" => "orbit_survey",
        "CP-S" => "static_studies",
        _ => "ui_sequences",
    }
}

fn path_poses(id: &str, seed: u64) -> Option<Vec<(String, PathPose)>> {
    match id {
        "CP-A" => Some(cpa_poses()),
        "CP-B" => Some(cpb_poses()),
        "CP-S" => Some(cps_poses(seed)),
        _ => None,
    }
}

/// The text of one path file. LF line endings, one pose per line, every number
/// the shortest decimal that reads back as the same `f32`.
fn path_file_text(id: &str, seed: u64) -> String {
    let mut s = String::new();
    if id == "CP-U" {
        s.push_str(&format!(
            "{{\n  \"schema\": \"{PATH_SCHEMA}\",\n  \"id\": \"CP-U\",\n  \"name\": \"ui_sequences\",\n  \"kind\": \"ui_sequences\",\n  \"scene\": {{ \"id\": \"S4\", \"dir\": \"S4_studio_ui\" }},\n  \"authoring\": \"Event-indexed, not a camera path: full-window captures at declared interaction steps (02_CAPTURE_HARNESS.md 4.3). Targets are CONFIG DEFAULT names; the recipe binds them to the live studio.\",\n"
        ));
        let sizes: Vec<String> = UI_SIZES.iter().map(|(w, h)| format!("[{w}, {h}]")).collect();
        s.push_str(&format!("  \"sizes\": [{}],\n  \"sequences\": [\n", sizes.join(", ")));
        for (si, (name, steps)) in UI_SEQUENCES.iter().enumerate() {
            s.push_str(&format!("    {{\n      \"name\": \"{name}\",\n      \"steps\": [\n"));
            for (i, (action, target)) in steps.iter().enumerate() {
                let comma = if i + 1 < steps.len() { "," } else { "" };
                s.push_str(&format!("        {{ \"step\": {i}, \"action\": {}, \"target\": {} }}{comma}\n", q(action), q(target)));
            }
            s.push_str(&format!("      ]\n    }}{}\n", if si + 1 < UI_SEQUENCES.len() { "," } else { "" }));
        }
        s.push_str("  ]\n}\n");
        return s;
    }
    let poses = path_poses(id, seed).unwrap_or_default();
    let role = if id == "CP-A" { "  \"role\": \"first_impression\",\n" } else { "" };
    s.push_str(&format!(
        "{{\n  \"schema\": \"{PATH_SCHEMA}\",\n  \"id\": \"{id}\",\n  \"name\": \"{}\",\n  \"kind\": \"poses\",\n{role}  \"scene\": {{ \"id\": \"{PATH_SCENE_ID}\", \"dir\": \"{PATH_SCENE_DIR}\" }},\n  \"seed\": {seed},\n  \"rate_hz\": {PATH_RATE_HZ},\n  \"ticks\": {},\n  \"units\": \"metres\",\n  \"rotation\": \"quaternion [x, y, z, w]; the camera looks along its local -Z with +Y up\",\n  \"authoring\": \"One enumerated pose per tick, no interpolation at replay. Values are CONFIG DEFAULT outputs of {BIN_NAME} paths --emit against {PATH_SCENE_DIR}; no render has judged the framing yet (TARGET).\",\n  \"poses\": [\n",
        path_name(id),
        poses.len()
    ));
    for (i, (label, p)) in poses.iter().enumerate() {
        let lab = if label.is_empty() { String::new() } else { format!("\"label\": {}, ", q(label)) };
        let comma = if i + 1 < poses.len() { "," } else { "" };
        s.push_str(&format!(
            "    {{ \"tick\": {i}, {lab}\"position\": [{}, {}, {}], \"rotation\": [{}, {}, {}, {}] }}{comma}\n",
            p.position[0], p.position[1], p.position[2], p.rotation[0], p.rotation[1], p.rotation[2], p.rotation[3]
        ));
    }
    s.push_str("  ]\n}\n");
    s
}

/// A pose path file read back: id, declared tick count, and the poses. A pose
/// must declare the tick equal to its index; the count must equal the list.
fn parse_pose_file(text: &str) -> Result<(String, usize, Vec<PathPose>), String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    if v.get("schema").and_then(|s| s.as_str()) != Some(PATH_SCHEMA) {
        return Err(format!("schema is not {PATH_SCHEMA}"));
    }
    if v.get("kind").and_then(|s| s.as_str()) != Some("poses") {
        return Err("kind is not \"poses\"".to_string());
    }
    let id = v.get("id").and_then(|s| s.as_str()).ok_or("no id")?.to_string();
    let declared = v.get("ticks").and_then(|n| n.as_u64()).ok_or("no ticks")? as usize;
    let list = v.get("poses").and_then(|p| p.as_array()).ok_or("no poses array")?;
    if list.len() != declared {
        return Err(format!("declares {declared} ticks but lists {} poses", list.len()));
    }
    let mut poses = Vec::with_capacity(list.len());
    for (i, p) in list.iter().enumerate() {
        if p.get("tick").and_then(|t| t.as_u64()) != Some(i as u64) {
            return Err(format!("pose {i} does not declare tick {i}"));
        }
        let nums = |key: &str, n: usize| -> Result<Vec<f32>, String> {
            let a = p.get(key).and_then(|a| a.as_array()).ok_or_else(|| format!("pose {i}: no {key}"))?;
            if a.len() != n {
                return Err(format!("pose {i}: {key} has {} numbers, wants {n}", a.len()));
            }
            a.iter()
                .map(|x| x.as_f64().filter(|f| f.is_finite()).map(|f| f as f32).ok_or_else(|| format!("pose {i}: {key} holds a bad number")))
                .collect()
        };
        let (pos, rot) = (nums("position", 3)?, nums("rotation", 4)?);
        let norm = rot.iter().map(|c| (*c as f64) * (*c as f64)).sum::<f64>().sqrt();
        if (norm - 1.0).abs() > 1.0e-5 {
            return Err(format!("pose {i}: rotation is not a unit quaternion (norm {norm})"));
        }
        poses.push(PathPose { position: [pos[0], pos[1], pos[2]], rotation: [rot[0], rot[1], rot[2], rot[3]] });
    }
    Ok((id, declared, poses))
}

/// CP-U: schema, the three sizes, and the four sequences with their step counts.
fn check_cpu_file(text: &str) -> Result<serde_json::Value, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    if v.get("schema").and_then(|s| s.as_str()) != Some(PATH_SCHEMA) || v.get("id").and_then(|s| s.as_str()) != Some("CP-U") {
        return Err("schema or id is wrong".to_string());
    }
    let sizes: Vec<(u64, u64)> = v
        .get("sizes")
        .and_then(|s| s.as_array())
        .ok_or("no sizes")?
        .iter()
        .filter_map(|p| Some((p.get(0)?.as_u64()?, p.get(1)?.as_u64()?)))
        .collect();
    let want: Vec<(u64, u64)> = UI_SIZES.iter().map(|(w, h)| (*w as u64, *h as u64)).collect();
    if sizes != want {
        return Err(format!("sizes are {sizes:?}, want {want:?}"));
    }
    let seqs = v.get("sequences").and_then(|s| s.as_array()).ok_or("no sequences")?;
    if seqs.len() != UI_SEQUENCES.len() {
        return Err(format!("{} sequences, want {}", seqs.len(), UI_SEQUENCES.len()));
    }
    let mut counts = serde_json::Map::new();
    for (seq, (name, steps)) in seqs.iter().zip(UI_SEQUENCES.iter()) {
        if seq.get("name").and_then(|n| n.as_str()) != Some(*name) {
            return Err(format!("a sequence is not {name}"));
        }
        let got = seq.get("steps").and_then(|s| s.as_array()).ok_or_else(|| format!("{name}: no steps"))?;
        if got.len() != steps.len() {
            return Err(format!("{name}: {} steps, want {}", got.len(), steps.len()));
        }
        for (i, st) in got.iter().enumerate() {
            if st.get("step").and_then(|n| n.as_u64()) != Some(i as u64) || st.get("action").and_then(|a| a.as_str()) != Some(steps[i].0) {
                return Err(format!("{name}: step {i} is not {}", steps[i].0));
            }
        }
        counts.insert(name.to_string(), serde_json::json!(got.len()));
    }
    Ok(serde_json::Value::Object(counts))
}

fn rows_sha256(rows: &[[u32; 7]]) -> String {
    let mut h = Sha256::new();
    for (i, r) in rows.iter().enumerate() {
        h.update((i as u32).to_le_bytes());
        for b in r {
            h.update(b.to_le_bytes());
        }
    }
    hex(&h.finalize())
}

/// The frozen tree hash of `dir_name` from the G1.03 freeze record.
fn frozen_scene_sha(dir_name: &str, explicit: Option<PathBuf>) -> Option<String> {
    let rel = Path::new("docs").join("PROMPTS").join("artifacts").join("G1.03").join("scene_freeze.json");
    let file = explicit.or_else(|| {
        let cwd = std::env::current_dir().ok()?;
        cwd.ancestors().map(|a| a.join(&rel)).find(|p| p.is_file())
    })?;
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    v.get("scenes")?
        .as_array()?
        .iter()
        .find(|r| r.get("space_dir").and_then(|d| d.as_str()) == Some(dir_name))?
        .get("sha256")?
        .as_str()
        .map(str::to_string)
}

/// A byte-identical copy of `space` for one engine launch, so no launch sees
/// what an earlier one wrote.
fn fresh_run_copy(space: &Path, label: &str) -> Result<PathBuf, String> {
    let name = space.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Space".to_string());
    let copy = std::env::temp_dir().join("eustress-harness-run").join(format!("paths-{label}")).join("Spaces").join(name);
    let _ = std::fs::remove_dir_all(&copy);
    let files = tree_files(space)?;
    write_scene_dir(&copy, &files, true)?;
    let (a, b) = (tree_digest(space)?, tree_digest(&copy)?);
    if a.sha256 != b.sha256 {
        return Err(format!("the copy of {} does not hash like the original", space.display()));
    }
    Ok(copy)
}

struct ReplayRun {
    rows: Vec<[u32; 7]>,
    declared: usize,
    readback_mismatches: usize,
    error: Option<String>,
}

/// Launch the engine on `run_space` armed with path `id`, wait for it to write
/// its pose log and exit, and read the log back.
fn run_replay(engine: &Path, run_space: &Path, id: &str, file: &Path, log: &Path, timeout_s: u64) -> Result<ReplayRun, String> {
    let _ = std::fs::remove_file(log);
    if let Some(parent) = log.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut child = std::process::Command::new(engine)
        .arg("--space")
        .arg(run_space)
        .env("EUSTRESS_CAMERA_PATH", id)
        .env("EUSTRESS_CAMERA_PATH_FILE", file)
        .env("EUSTRESS_CAMERA_PATH_LOG", log)
        .env("EUSTRESS_CAMERA_PATH_EXIT", "1")
        .env("EUSTRESS_WINDOW_SIZE", "1280x720")
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", engine.display()))?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.to_string(),
            Ok(None) if start.elapsed() > Duration::from_secs(timeout_s) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("the engine ran past {timeout_s} s without finishing {id}; stopped it"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(250)),
            Err(e) => return Err(format!("waiting on the engine: {e}")),
        }
    };
    let text = std::fs::read_to_string(log).map_err(|e| format!("engine exited ({status}) and left no pose log at {}: {e}", log.display()))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("pose log is not JSON: {e}"))?;
    if v.get("schema").and_then(|s| s.as_str()) != Some(PATH_LOG_SCHEMA) {
        return Err(format!("pose log schema is not {PATH_LOG_SCHEMA}"));
    }
    let mut rows = Vec::new();
    for (i, r) in v.get("rows").and_then(|r| r.as_array()).ok_or("pose log has no rows")?.iter().enumerate() {
        let a = r.as_array().filter(|a| a.len() == 7).ok_or_else(|| format!("log row {i} is not seven numbers"))?;
        let mut row = [0u32; 7];
        for (slot, n) in row.iter_mut().zip(a) {
            *slot = n.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| format!("log row {i} holds a bad bit pattern"))?;
        }
        rows.push(row);
    }
    Ok(ReplayRun {
        rows,
        declared: v.get("ticks_declared").and_then(|n| n.as_u64()).unwrap_or(0) as usize,
        readback_mismatches: v.get("readback_mismatches").and_then(|m| m.as_array()).map(|m| m.len()).unwrap_or(usize::MAX),
        error: v.get("error").and_then(|e| e.as_str()).map(str::to_string),
    })
}

fn paths(args: &[String]) -> i32 {
    let seed = value(args, "--seed").and_then(|s| s.parse::<u64>().ok()).unwrap_or(DEFAULT_SEED);
    if flag(args, "--emit") {
        let dir = absolute(&value(args, "--paths-dir").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("spaces/harness/paths")));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("ERROR: cannot create {}: {e}", dir.display());
            return 3;
        }
        for id in PATH_IDS {
            let text = path_file_text(id, seed);
            let file = dir.join(format!("{id}.json"));
            if let Err(e) = std::fs::write(&file, text.as_bytes()) {
                eprintln!("ERROR: cannot write {}: {e}", file.display());
                return 3;
            }
            println!("{id} {} sha256={}", file.display(), hex(&Sha256::digest(text.as_bytes())));
        }
        return 0;
    }
    if flag(args, "--verify") {
        return paths_verify(args, seed);
    }
    eprintln!("ERROR: paths needs --emit [--paths-dir <dir>] or --verify --out <json> --space <dir> --paths CP-A,CP-B,CP-S");
    2
}

fn paths_verify(args: &[String], seed: u64) -> i32 {
    let (Some(out), Some(space), Some(list)) = (value(args, "--out").map(PathBuf::from), value(args, "--space").map(PathBuf::from), value(args, "--paths")) else {
        eprintln!("ERROR: --out <json>, --space <dir> and --paths CP-A,CP-B,CP-S are required");
        return 2;
    };
    let ids: Vec<String> = list.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
    if ids.is_empty() || ids.iter().any(|i| !matches!(i.as_str(), "CP-A" | "CP-B" | "CP-S")) {
        eprintln!("ERROR: --paths takes CP-A, CP-B and CP-S (CP-U is event-indexed; its schema is always checked, never replayed)");
        return 2;
    }
    let out = absolute(&out);
    let space = absolute(&space);
    if !space.join("space.toml").is_file() {
        eprintln!("ERROR: {} is not a Space (no space.toml); run the generator first", space.display());
        return 2;
    }
    let paths_dir = absolute(&value(args, "--paths-dir").map(PathBuf::from).unwrap_or_else(|| space.parent().map(|p| p.join("paths")).unwrap_or_else(|| PathBuf::from("paths"))));
    let timeout = value(args, "--timeout-s").and_then(|v| v.parse::<u64>().ok()).unwrap_or(PATH_ENGINE_TIMEOUT_S);
    let space_name = space.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();

    let mut errors: Vec<String> = Vec::new();

    // 1. The four files are the generator's output, byte for byte.
    let mut file_rows = serde_json::Map::new();
    let mut texts: BTreeMap<&str, String> = BTreeMap::new();
    for id in PATH_IDS {
        let file = paths_dir.join(format!("{id}.json"));
        match std::fs::read(&file) {
            Ok(bytes) => {
                let sha = hex(&Sha256::digest(&bytes));
                let matches_generator = bytes == path_file_text(id, seed).as_bytes();
                if !matches_generator {
                    errors.push(format!("{id}: {} differs from the generator's output (run `{BIN_NAME} paths --emit`)", file.display()));
                }
                file_rows.insert(id.to_string(), serde_json::json!({ "path": file.to_string_lossy(), "sha256": sha, "bytes": bytes.len(), "matches_generator": matches_generator }));
                texts.insert(id, String::from_utf8_lossy(&bytes).to_string());
            }
            Err(e) => errors.push(format!("{id}: cannot read {}: {e}", file.display())),
        }
    }

    // 2. CP-U parses and declares all four sequences with their step counts.
    let cpu = texts.get("CP-U").map(|t| check_cpu_file(t));
    let cpu_steps = match cpu {
        Some(Ok(counts)) => {
            println!("CP-U schema ok sequences={counts}");
            Some(counts)
        }
        Some(Err(e)) => {
            errors.push(format!("CP-U: {e}"));
            None
        }
        None => None,
    };

    // 3. The pose files parse, declare the counts of 02 4.1, and read back as
    //    the very f32 values the generator computed.
    let mut declared: BTreeMap<String, (usize, Vec<PathPose>)> = BTreeMap::new();
    for id in &ids {
        let Some(text) = texts.get(id.as_str()) else { continue };
        match parse_pose_file(text) {
            Ok((fid, n, poses)) => {
                let want_ticks = match id.as_str() {
                    "CP-A" => Some(CPA_TICKS),
                    "CP-B" => Some(CPB_TICKS),
                    _ => Some(CPS_TICKS),
                };
                if &fid != id || want_ticks != Some(n) {
                    errors.push(format!("{id}: file declares id {fid} and {n} ticks, want {id} and {}", want_ticks.unwrap_or(0)));
                }
                let regenerated: Vec<[u32; 7]> = path_poses(id, seed).unwrap_or_default().iter().map(|(_, p)| p.bits()).collect();
                if regenerated != poses.iter().map(PathPose::bits).collect::<Vec<_>>() {
                    errors.push(format!("{id}: the file's numbers do not read back as the generator's f32 values"));
                }
                declared.insert(id.clone(), (n, poses));
            }
            Err(e) => errors.push(format!("{id}: {e}")),
        }
    }

    // 4. The scene is the frozen one.
    let scene_sha = tree_digest(&space).map(|d| d.sha256).ok();
    let frozen = frozen_scene_sha(&space_name, value(args, "--freeze").map(PathBuf::from));
    let scene_matches_frozen = match (&scene_sha, &frozen) {
        (Some(a), Some(b)) => Some(a == b),
        _ => None,
    };
    if scene_matches_frozen == Some(false) {
        errors.push(format!("{space_name} does not hash like its G1.03 frozen record"));
    }
    if space_name != PATH_SCENE_DIR {
        errors.push(format!("the paths are authored against {PATH_SCENE_DIR}; --space is {space_name}"));
    }

    // 5. Replay each path twice, each launch a fresh engine on a fresh copy.
    let exe_name = format!("eustress-engine{}", std::env::consts::EXE_SUFFIX);
    let engine = value(args, "--engine").map(PathBuf::from).or_else(|| std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(&exe_name))));
    let mut rows_out = Vec::new();
    let (mut identical_count, mut tick_mismatches) = (0usize, 0usize);
    let mut extra_failures = 0usize;
    match engine.as_ref().filter(|e| e.is_file()) {
        None => errors.push(format!("{exe_name} not found beside this binary; build it first (cargo build --release --bin eustress-engine)")),
        Some(engine) => {
            for id in &ids {
                let Some((n_declared, poses)) = declared.get(id) else {
                    errors.push(format!("{id}: not replayed, its file did not validate"));
                    continue;
                };
                let file = paths_dir.join(format!("{id}.json"));
                let want_rows: Vec<[u32; 7]> = poses.iter().map(PathPose::bits).collect();
                let mut runs: Vec<ReplayRun> = Vec::new();
                for label in ["a", "b"] {
                    let run_space = if flag(args, "--in-place") {
                        Ok(space.clone())
                    } else {
                        fresh_run_copy(&space, &format!("{id}-{label}"))
                    };
                    let log = std::env::temp_dir().join("eustress-harness-run").join(format!("paths-{id}-{label}.log.json"));
                    match run_space.and_then(|rs| run_replay(engine, &rs, id, &file, &log, timeout)) {
                        Ok(run) => runs.push(run),
                        Err(e) => {
                            errors.push(format!("{id} run {label}: {e}"));
                            break;
                        }
                    }
                }
                if runs.len() != 2 {
                    extra_failures += 1;
                    continue;
                }
                let (a, b) = (&runs[0], &runs[1]);
                let identical = a.rows == b.rows;
                let ticks_ok = a.rows.len() == *n_declared && b.rows.len() == *n_declared && a.declared == *n_declared && b.declared == *n_declared;
                let declared_ok = a.rows == want_rows && b.rows == want_rows;
                let (sha_a, sha_b) = (rows_sha256(&a.rows), rows_sha256(&b.rows));
                println!(
                    "{id} ticks_declared={n_declared} ticks_replayed={} run_a_sha256={sha_a} run_b_sha256={sha_b} identical={identical}",
                    a.rows.len().min(b.rows.len())
                );
                if identical {
                    identical_count += 1;
                } else if let Some(i) = a.rows.iter().zip(&b.rows).position(|(x, y)| x != y) {
                    errors.push(format!("{id}: the runs first differ at tick {i}: {:?} vs {:?}", a.rows[i], b.rows[i]));
                }
                if !ticks_ok {
                    tick_mismatches += 1;
                    errors.push(format!("{id}: tick count: declared {n_declared}, replayed {} and {}", a.rows.len(), b.rows.len()));
                }
                if !declared_ok || a.readback_mismatches != 0 || b.readback_mismatches != 0 || a.error.is_some() || b.error.is_some() {
                    extra_failures += 1;
                    errors.push(format!(
                        "{id}: readback differs from the declared poses (matches_declared={declared_ok}, readback_mismatches {} and {}, engine errors {:?} and {:?})",
                        a.readback_mismatches, b.readback_mismatches, a.error, b.error
                    ));
                }
                let pose_count = if id == "CP-S" { serde_json::json!(poses.len()) } else { serde_json::Value::Null };
                rows_out.push(serde_json::json!({
                    "id": id,
                    "name": path_name(id),
                    "tick_range": [0, n_declared - 1],
                    "ticks_declared": n_declared,
                    "ticks_replayed": [a.rows.len(), b.rows.len()],
                    "run_a_sha256": sha_a,
                    "run_b_sha256": sha_b,
                    "identical": identical,
                    "matches_declared_poses": declared_ok,
                    "readback_mismatches": [a.readback_mismatches, b.readback_mismatches],
                    "pose_count": pose_count,
                }));
            }
        }
    }
    println!("paths={} identical={} tick_mismatches={}", ids.len(), identical_count, tick_mismatches);
    for e in &errors {
        eprintln!("error: {e}");
    }

    let (commit, source_clean) = git_state();
    let report = serde_json::json!({
        "item": "G1.04",
        "tool": format!("{BIN_NAME} paths --verify"),
        "paths": rows_out,
        "path_files": file_rows,
        "cp_u_sequence_steps": cpu_steps,
        "scene": {
            "dir": space_name,
            "scene_id": PATH_SCENE_ID,
            "tree_sha256": scene_sha,
            "frozen_sha256": frozen,
            "matches_frozen": scene_matches_frozen,
            "ran_from_copy": !flag(args, "--in-place"),
        },
        "tick_source": "FixedUpdate runs since the path started (Time<Fixed>, 60 Hz)",
        "seed": seed,
        "commit": commit,
        "source_matches_commit": source_clean,
        "reference_machine_id": std::env::var("EUSTRESS_REFERENCE_MACHINE_ID").ok().filter(|s| !s.is_empty()),
        "summary": { "paths": ids.len(), "identical": identical_count, "tick_mismatches": tick_mismatches },
        "errors": errors,
    });
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&out, serde_json::to_string_pretty(&report).unwrap_or_default() + "\n") {
        eprintln!("ERROR: cannot write {}: {e}", out.display());
        return 5;
    }
    let _ = std::io::stdout().flush();
    if errors.is_empty() && identical_count == ids.len() && tick_mismatches == 0 && extra_failures == 0 {
        0
    } else {
        1
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// tick-selftest (G1.06): captures fire at declared simulation tick indices
// ─────────────────────────────────────────────────────────────────────────────
//
//   harness-scenes tick-selftest --space <dir> --ticks 0,60,300,600 --width W
//       --height H --slow-run-frame-cap N --compression-refusal-check 2.0
//       --out <json> [--timeout-s N] [--engine <exe>] [--freeze <json>]
//
// Three launches of the windowed engine, each on its own byte-identical copy of
// the Space. The engine arms itself from `EUSTRESS_TICK_CAPTURE*` (ai_camera.rs),
// opens Play, captures at each tick, writes its log and exits.

/// CONFIG DEFAULT: seconds one engine launch may take.
const TICK_ENGINE_TIMEOUT_S: u64 = 600;
/// CONFIG DEFAULT: how far above the cap the slowed run's measured frame rate
/// may sit and still count as capped.
const TICK_CAP_SLACK: f64 = 1.15;

struct TickRun {
    exit: Option<i32>,
    wall_s: f64,
    emitted: Vec<u64>,
    frames_on_disk: usize,
    mean_fps: f64,
    refused: Option<String>,
    error: Option<String>,
    log: Option<serde_json::Value>,
}

fn tick_list(ticks: &[u64]) -> String {
    format!("[{}]", ticks.iter().map(u64::to_string).collect::<Vec<_>>().join(","))
}

/// One engine launch on a fresh copy of `space`, armed for `ticks`.
#[allow(clippy::too_many_arguments)]
fn run_tick_engine(
    engine: &Path,
    space: &Path,
    label: &str,
    ticks: &[u64],
    size: (u32, u32),
    frame_cap: Option<f64>,
    time_scale: Option<f64>,
    timeout_s: u64,
) -> TickRun {
    let mut run = TickRun { exit: None, wall_s: 0.0, emitted: Vec::new(), frames_on_disk: 0, mean_fps: 0.0, refused: None, error: None, log: None };
    let root = std::env::temp_dir().join("eustress-harness-run").join(format!("tick-{label}"));
    let dir = root.join("captures");
    let log_path = root.join("tick_log.json");
    let _ = std::fs::remove_dir_all(&root);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        run.error = Some(format!("cannot create {}: {e}", dir.display()));
        return run;
    }
    let copy = match fresh_run_copy(space, &format!("tick-{label}")) {
        Ok(c) => c,
        Err(e) => {
            run.error = Some(e);
            return run;
        }
    };
    let mut cmd = std::process::Command::new(engine);
    cmd.arg("--space")
        .arg(&copy)
        .env("EUSTRESS_TICK_CAPTURE", ticks.iter().map(u64::to_string).collect::<Vec<_>>().join(","))
        .env("EUSTRESS_TICK_CAPTURE_DIR", &dir)
        .env("EUSTRESS_TICK_CAPTURE_SIZE", format!("{}x{}", size.0, size.1))
        .env("EUSTRESS_TICK_CAPTURE_LOG", &log_path)
        .env("EUSTRESS_TICK_CAPTURE_EXIT", "1")
        .env("EUSTRESS_WINDOW_SIZE", "1280x720");
    if let Some(cap) = frame_cap {
        cmd.env("EUSTRESS_TICK_CAPTURE_FRAME_CAP", cap.to_string());
    }
    if let Some(scale) = time_scale {
        cmd.env("EUSTRESS_TICK_CAPTURE_TIME_SCALE", format!("{scale:?}"));
    }
    let started = Instant::now();
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            run.error = Some(format!("cannot start {}: {e}", engine.display()));
            return run;
        }
    };
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                run.exit = Some(status.code().unwrap_or(-1));
                break;
            }
            Ok(None) if started.elapsed() > Duration::from_secs(timeout_s) => {
                let _ = child.kill();
                let _ = child.wait();
                run.error = Some(format!("the engine ran past {timeout_s} s; stopped it"));
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(250)),
            Err(e) => {
                run.error = Some(format!("waiting on the engine: {e}"));
                break;
            }
        }
    }
    run.wall_s = started.elapsed().as_secs_f64();
    run.frames_on_disk = std::fs::read_dir(&dir)
        .map(|rd| rd.filter_map(|e| e.ok()).filter(|e| e.path().extension().is_some_and(|x| x == "png")).count())
        .unwrap_or(0);
    match std::fs::read_to_string(&log_path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).map_err(|e| e.to_string())) {
        Ok(v) => {
            if v.get("schema").and_then(|s| s.as_str()) != Some("eustress.tick_capture_log/1") {
                run.error.get_or_insert_with(|| "the tick log has the wrong schema".to_string());
            }
            run.emitted = v.get("ticks_emitted").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|n| n.as_u64()).collect()).unwrap_or_default();
            run.mean_fps = v.get("mean_fps").and_then(|n| n.as_f64()).unwrap_or(0.0);
            run.refused = v.get("refused").and_then(|s| s.as_str()).map(str::to_string);
            if let Some(e) = v.get("error").and_then(|s| s.as_str()) {
                run.error.get_or_insert_with(|| e.to_string());
            }
            run.log = Some(v);
        }
        Err(e) => {
            run.error.get_or_insert_with(|| format!("no readable tick log at {}: {e}", log_path.display()));
        }
    }
    run
}

fn tick_selftest(args: &[String]) -> i32 {
    let (Some(space), Some(ticks_arg), Some(out)) = (value(args, "--space").map(PathBuf::from), value(args, "--ticks"), value(args, "--out").map(PathBuf::from)) else {
        eprintln!("ERROR: --space <dir>, --ticks 0,60,300,600 and --out <json> are required");
        return 2;
    };
    let mut ticks: Vec<u64> = Vec::new();
    for item in ticks_arg.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match item.parse::<u64>() {
            Ok(t) if ticks.last().map_or(true, |l| t > *l) => ticks.push(t),
            _ => {
                eprintln!("ERROR: --ticks must be strictly increasing whole numbers; got {item:?}");
                return 2;
            }
        }
    }
    let (Some(width), Some(height)) = (value(args, "--width").and_then(|v| v.parse::<u32>().ok()), value(args, "--height").and_then(|v| v.parse::<u32>().ok())) else {
        eprintln!("ERROR: --width W and --height H are required");
        return 2;
    };
    let (Some(cap), Some(scale)) = (
        value(args, "--slow-run-frame-cap").and_then(|v| v.parse::<f64>().ok()).filter(|c| *c > 0.0),
        value(args, "--compression-refusal-check").and_then(|v| v.parse::<f64>().ok()).filter(|s| *s != 1.0),
    ) else {
        eprintln!("ERROR: --slow-run-frame-cap N (frames per second, above 0) and --compression-refusal-check S (a time scale other than 1.0) are required");
        return 2;
    };
    if ticks.is_empty() {
        eprintln!("ERROR: --ticks is empty");
        return 2;
    }
    let out = absolute(&out);
    let space = absolute(&space);
    if !space.join("space.toml").is_file() {
        eprintln!("ERROR: {} is not a Space (no space.toml); run the generator first", space.display());
        return 2;
    }
    let timeout = value(args, "--timeout-s").and_then(|v| v.parse::<u64>().ok()).unwrap_or(TICK_ENGINE_TIMEOUT_S);
    let space_name = space.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut errors: Vec<String> = Vec::new();

    let scene_sha = tree_digest(&space).map(|d| d.sha256).ok();
    let frozen = frozen_scene_sha(&space_name, value(args, "--freeze").map(PathBuf::from));
    let scene_matches_frozen = match (&scene_sha, &frozen) {
        (Some(a), Some(b)) => Some(a == b),
        _ => None,
    };
    if scene_matches_frozen == Some(false) {
        errors.push(format!("{space_name} does not hash like its G1.03 frozen record"));
    }

    let exe_name = format!("eustress-engine{}", std::env::consts::EXE_SUFFIX);
    let engine = value(args, "--engine").map(PathBuf::from).or_else(|| std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(&exe_name))));
    let Some(engine) = engine.filter(|e| e.is_file()) else {
        eprintln!("ERROR: {exe_name} not found beside this binary; build it first (cargo build --release --bin eustress-engine)");
        return 2;
    };

    let normal = run_tick_engine(&engine, &space, "normal", &ticks, (width, height), None, None, timeout);
    let slowed = run_tick_engine(&engine, &space, "slowed", &ticks, (width, height), Some(cap), None, timeout);
    let compression = run_tick_engine(&engine, &space, "compression", &ticks, (width, height), None, Some(scale), timeout);

    let want_refusal = format!("capture refused: sim.time_scale={scale:?}, required 1.0");
    let normal_ok = normal.exit == Some(0) && normal.emitted == ticks && normal.frames_on_disk == ticks.len();
    let slowed_ok = slowed.exit == Some(0) && slowed.emitted == ticks && slowed.frames_on_disk == ticks.len();
    let tick_sets_equal = normal.emitted == slowed.emitted;
    // Confirm the cap took effect: the slowed run's measured frame rate is at
    // most the cap (within slack) and below the normal run's.
    let cap_took_effect = slowed.mean_fps > 0.0 && slowed.mean_fps <= cap * TICK_CAP_SLACK && slowed.mean_fps < normal.mean_fps;
    let compression_ok = compression.exit.is_some_and(|c| c != 0)
        && compression.frames_on_disk == 0
        && compression.emitted.is_empty()
        && compression.refused.as_deref() == Some(want_refusal.as_str());

    println!("normal   ticks_requested={} ticks_emitted={} frames={}", tick_list(&ticks), tick_list(&normal.emitted), normal.frames_on_disk);
    println!("slowed   ticks_requested={} ticks_emitted={} frames={}", tick_list(&ticks), tick_list(&slowed.emitted), slowed.frames_on_disk);
    println!("tick_sets_equal={tick_sets_equal}");
    println!(
        "compression_run exit={} frames_emitted={} error={:?}",
        compression.exit.map(|c| c.to_string()).unwrap_or_else(|| "none".to_string()),
        compression.frames_on_disk,
        compression.refused.clone().unwrap_or_default()
    );
    println!(
        "wall_s normal={:.1} slowed={:.1} mean_fps normal={:.1} slowed={:.1} frame_cap={cap} cap_took_effect={cap_took_effect}",
        normal.wall_s, slowed.wall_s, normal.mean_fps, slowed.mean_fps
    );
    for (name, run) in [("normal", &normal), ("slowed", &slowed), ("compression", &compression)] {
        if let Some(e) = &run.error {
            if name != "compression" {
                errors.push(format!("{name}: {e}"));
            }
        }
    }
    if !normal_ok {
        errors.push("normal run: exit not 0, or emitted ticks differ from the requested list, or the PNG count is wrong".to_string());
    }
    if !slowed_ok {
        errors.push("slowed run: exit not 0, or emitted ticks differ from the requested list, or the PNG count is wrong".to_string());
    }
    if !tick_sets_equal {
        errors.push("the normal and slowed runs emitted different tick sets".to_string());
    }
    if !cap_took_effect {
        errors.push(format!("the frame cap did not take effect: slowed {:.1} fps, normal {:.1} fps, cap {cap}", slowed.mean_fps, normal.mean_fps));
    }
    if !compression_ok {
        errors.push(format!("the compression run must exit non-zero with no frame and the error {want_refusal:?}"));
    }
    for e in &errors {
        eprintln!("error: {e}");
    }

    let from_log = |run: &TickRun, key: &str| run.log.as_ref().and_then(|l| l.get(key)).cloned().unwrap_or(serde_json::Value::Null);
    let (commit, source_clean) = git_state();
    let row = |run: &TickRun| {
        serde_json::json!({
            "exit": run.exit,
            "ticks_emitted": run.emitted,
            "frames": run.frames_on_disk,
            "wall_s": run.wall_s,
            "mean_fps": run.mean_fps,
            "refused": run.refused,
            "error": run.error,
            "engine_log": run.log,
        })
    };
    let report = serde_json::json!({
        "item": "G1.06",
        "tool": format!("{BIN_NAME} tick-selftest"),
        "tick_source": "physics steps executed since the Play run began (PhysicsStepCounter minus StepCountRun.start); clock.tick_count is logged beside it",
        "ticks_requested": ticks,
        "normal": row(&normal),
        "slowed": row(&slowed),
        "frame_cap_used": cap,
        "cap_took_effect": cap_took_effect,
        "tick_sets_equal": tick_sets_equal,
        "compression_run": {
            "time_scale": scale,
            "exit": compression.exit,
            "frames_emitted": compression.frames_on_disk,
            "error_literal": compression.refused,
            "expected_error_literal": want_refusal,
            "run": row(&compression),
        },
        "scene": {
            "dir": space_name,
            "tree_sha256": scene_sha,
            "frozen_sha256": frozen,
            "matches_frozen": scene_matches_frozen,
            "ran_from_copy": true,
        },
        "width": width,
        "height": height,
        "fixed_hz": from_log(&normal, "fixed_hz"),
        "fixed_timestep_s": from_log(&normal, "fixed_timestep_s"),
        "substeps": from_log(&normal, "substeps"),
        "reference_machine_id": std::env::var("EUSTRESS_REFERENCE_MACHINE_ID").ok().filter(|s| !s.is_empty()),
        "commit": commit,
        "source_matches_commit": source_clean,
        "errors": errors,
    });
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&out, serde_json::to_string_pretty(&report).unwrap_or_default() + "\n") {
        eprintln!("ERROR: cannot write {}: {e}", out.display());
        return 5;
    }
    let _ = std::io::stdout().flush();
    if errors.is_empty() && normal_ok && slowed_ok && tick_sets_equal && cap_took_effect && compression_ok {
        0
    } else {
        1
    }
}

/// Every file under `root`, keyed by its `/`-separated relative path.
fn tree_files(root: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut all = BTreeMap::new();
    collect_files(root, "", &mut all)?;
    Ok(all)
}
