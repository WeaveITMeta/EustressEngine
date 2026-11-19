//! The world digest: how the census decides, mechanically, whether a
//! mutating call had an observable effect.
//!
//! A digest is a map from component to hash:
//!
//! * `bridge:<method>`: the engine's answer to a fixed set of bridge reads
//!   (`ecs.query` over every class, `oplog.tail` of 32, `state.get`,
//!   `physics.get`, `sim.state`, `terrain.stats`, `snapshot.list`,
//!   `data.bindings`, `ui.modes`), with volatile keys (clocks, frame counters)
//!   removed before hashing. A read that fails hashes its error text.
//! * `fs:<relative path>`: size and content of every file under the fixture
//!   workspace, minus the engine's own liveness files.
//!
//! Before the census starts, the digest is taken several times with no call
//! in between; any component that changes on its own is volatile and left
//! out of every comparison. The volatile set is written into the artifact.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::util::{fnv1a64, fnv1a64_extend};

/// Component name to hash.
pub type Digest = BTreeMap<String, u64>;

/// The bridge reads, as (method, params).
pub fn bridge_reads() -> Vec<(&'static str, Value)> {
    vec![
        ("ecs.query", json!({ "limit": 100000 })),
        ("oplog.tail", json!({ "limit": 32 })),
        ("state.get", json!({})),
        ("physics.get", json!({})),
        ("sim.state", json!({})),
        ("terrain.stats", json!({})),
        ("snapshot.list", json!({})),
        ("data.bindings", json!({})),
        ("ui.modes", json!({})),
    ]
}

/// Keys removed (recursively) from a bridge answer before hashing.
pub const VOLATILE_KEYS: &[&str] = &[
    "fps",
    "frame",
    "frames",
    "frame_count",
    "frame_time_ms",
    "tick",
    "tick_count",
    "time",
    "timestamp",
    "elapsed",
    "elapsed_ms",
    "elapsed_s",
    "uptime",
    "uptime_s",
    "age",
    "age_ms",
    "age_s",
    "now",
    "captured_at",
    "wall_clock",
    "dt",
    "delta",
];

/// File names never hashed: the engine's liveness and telemetry files.
pub const EXCLUDED_FILE_NAMES: &[&str] = &[
    "engine.port",
    "lsp.port",
    "runtime-snapshot.json",
    "telemetry.jsonl",
    "sim-commands.jsonl",
    "snapshot.json",
];

/// Directory names never walked.
pub const EXCLUDED_DIR_NAMES: &[&str] = &["instances", "target"];

/// Files above this size hash by size and modification time only.
const FULL_HASH_LIMIT: u64 = 64 * 1024 * 1024;

/// What to digest.
#[derive(Debug, Clone)]
pub struct DigestConfig {
    pub universe: PathBuf,
    pub fs_root: PathBuf,
    pub read_timeout: Duration,
    /// Paths (relative to `fs_root`, forward slashes) of files the census
    /// itself writes, never hashed.
    pub own_files: BTreeSet<String>,
}

/// One digest.
pub fn take(cfg: &DigestConfig) -> Digest {
    let mut d = Digest::new();
    for (method, params) in bridge_reads() {
        let h = match eustress_bridge_client::call_engine_with_timeout(&cfg.universe, method, params, cfg.read_timeout) {
            Ok(mut v) => {
                normalize(&mut v);
                fnv1a64(serde_json::to_string(&v).unwrap_or_default().as_bytes())
            }
            Err(e) => fnv1a64(format!("err:{e}").as_bytes()),
        };
        d.insert(format!("bridge:{method}"), h);
    }
    walk(&cfg.fs_root, &cfg.fs_root, cfg, &mut d);
    d
}

/// Remove volatile keys recursively.
pub fn normalize(v: &mut Value) {
    match v {
        Value::Object(m) => {
            m.retain(|k, _| !VOLATILE_KEYS.contains(&k.to_ascii_lowercase().as_str()));
            for (_, sub) in m.iter_mut() {
                normalize(sub);
            }
        }
        Value::Array(a) => {
            for sub in a.iter_mut() {
                normalize(sub);
            }
        }
        _ => {}
    }
}

fn rel_key(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn walk(root: &Path, dir: &Path, cfg: &DigestConfig, out: &mut Digest) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            if !EXCLUDED_DIR_NAMES.contains(&name.as_str()) {
                walk(root, &path, cfg, out);
            }
            continue;
        }
        if EXCLUDED_FILE_NAMES.contains(&name.as_str()) || name.ends_with(".token") {
            continue;
        }
        let key = rel_key(root, &path);
        if cfg.own_files.contains(&key) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let len = meta.len();
        let mut h = fnv1a64(&len.to_le_bytes());
        if len <= FULL_HASH_LIMIT {
            match std::fs::read(&path) {
                Ok(bytes) => h = fnv1a64_extend(h, &bytes),
                Err(_) => h = fnv1a64_extend(h, b"unreadable"),
            }
        } else if let Ok(m) = meta.modified() {
            let nanos = m.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            h = fnv1a64_extend(h, &nanos.to_le_bytes());
        }
        out.insert(format!("fs:{key}"), h);
    }
}

/// Components that differ between two digests (added, removed or changed),
/// minus the volatile set.
pub fn changed(a: &Digest, b: &Digest, volatile: &BTreeSet<String>) -> Vec<String> {
    let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    keys.into_iter()
        .filter(|k| !volatile.contains(*k))
        .filter(|k| a.get(*k) != b.get(*k))
        .cloned()
        .collect()
}

/// Take digests until two in a row agree (outside the volatile set), at most
/// `max` long. Returns the last digest and whether it settled.
pub fn settle(cfg: &DigestConfig, volatile: &BTreeSet<String>, interval: Duration, max: Duration) -> (Digest, bool) {
    let start = Instant::now();
    let mut prev = take(cfg);
    loop {
        if start.elapsed() >= max {
            return (prev, false);
        }
        std::thread::sleep(interval);
        let next = take(cfg);
        if changed(&prev, &next, volatile).is_empty() {
            return (next, true);
        }
        prev = next;
    }
}

/// Take `samples` digests `gap` apart with no call in between; every
/// component that moved is volatile.
pub fn calibrate(cfg: &DigestConfig, samples: usize, gap: Duration) -> BTreeSet<String> {
    let mut volatile = BTreeSet::new();
    let empty = BTreeSet::new();
    let mut prev = take(cfg);
    for _ in 1..samples.max(2) {
        std::thread::sleep(gap);
        let next = take(cfg);
        volatile.extend(changed(&prev, &next, &empty));
        prev = next;
    }
    volatile
}

/// The method statement written into the artifact.
pub fn method_statement() -> String {
    let reads: Vec<String> = bridge_reads().into_iter().map(|(m, p)| format!("{m}({p})")).collect();
    format!(
        "FNV-1a 64 per component over [{}] with volatile keys {:?} removed, plus size+content of every file under the fixture workspace except {:?} and *.token; \
         a component that changed during a no-call calibration is excluded; effect = any non-volatile component differs between a settled digest before the call and a settled digest after it",
        reads.join(", "),
        VOLATILE_KEYS,
        EXCLUDED_FILE_NAMES
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volatile_keys_are_removed_at_every_depth() {
        let mut v = json!({ "fps": 60, "world": { "tick": 9, "parts": [{ "name": "A", "timestamp": 1 }] } });
        normalize(&mut v);
        assert_eq!(v, json!({ "world": { "parts": [{ "name": "A" }] } }));
    }

    #[test]
    fn changes_ignore_the_volatile_set() {
        let mut a = Digest::new();
        a.insert("fs:x".into(), 1);
        a.insert("bridge:state.get".into(), 5);
        let mut b = a.clone();
        b.insert("bridge:state.get".into(), 6);
        b.insert("fs:y".into(), 2);
        let mut vol = BTreeSet::new();
        assert_eq!(changed(&a, &b, &vol), vec!["bridge:state.get".to_string(), "fs:y".to_string()]);
        vol.insert("bridge:state.get".to_string());
        assert_eq!(changed(&a, &b, &vol), vec!["fs:y".to_string()]);
    }

    #[test]
    fn the_file_walk_sees_writes_and_skips_liveness_files() {
        let root = std::env::temp_dir().join(format!("eus_census_digest_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".eustress")).unwrap();
        std::fs::write(root.join(".eustress").join("engine.port"), "1").unwrap();
        std::fs::write(root.join("a.txt"), "one").unwrap();
        let cfg = DigestConfig {
            universe: root.clone(),
            fs_root: root.clone(),
            read_timeout: Duration::from_millis(10),
            own_files: BTreeSet::new(),
        };
        let mut d1 = Digest::new();
        walk(&root, &root, &cfg, &mut d1);
        assert!(d1.contains_key("fs:a.txt"));
        assert!(!d1.keys().any(|k| k.ends_with("engine.port")));
        std::fs::write(root.join("a.txt"), "two").unwrap();
        let mut d2 = Digest::new();
        walk(&root, &root, &cfg, &mut d2);
        assert_eq!(changed(&d1, &d2, &BTreeSet::new()), vec!["fs:a.txt".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
