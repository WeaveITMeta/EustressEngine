//! # session-baseline — compute the crash-free-session rate from local records
//!
//! Reads the append-only session ledger the engine's session beacon writes
//! (`<telemetry>/sessions/*.jsonl`, see
//! `eustress_engine::usage_telemetry`) and emits one JSON object describing
//! every session this machine has run: how many there were, how many ended
//! uncleanly, the resulting crash-free rate, and the histogram of end reasons.
//!
//! ```text
//! session-baseline [--telemetry-dir <dir>] [--out <file>]
//! ```
//!
//! Everything is computed from **local** records. Nothing is fetched from the
//! telemetry Worker, so the number reproduces on a machine with no network —
//! which is the point: the uploaded aggregate is a convenience, the ledger on
//! disk is the evidence.
//!
//! ## What counts as unclean
//!
//! The vocabulary is not this binary's to invent. `EndReason` in the engine's
//! `usage_telemetry` module owns it, and this reader asks that enum:
//! `clean` is the only clean outcome, and a reason string this build does not
//! recognise (written by a newer engine) is counted as **unclean** and passed
//! through to the histogram verbatim. A reader can therefore never turn a
//! future fault class into a passing session by not knowing about it.
//!
//! A `start` record with no `end` record is counted as `orphaned` — the same
//! reason the engine itself assigns when it reconciles that start on a later
//! launch. The two paths agree by construction. The count of such
//! not-yet-reconciled starts is reported separately as
//! `sessions_unreconciled_at_read`; run this with no engine running, or the
//! live session will be one of them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eustress_engine::usage_telemetry::{EndReason, SESSION_LEDGER_FILE};

const USAGE: &str = "\
session-baseline — crash-free-session rate from the local session ledger

USAGE:
    session-baseline [--telemetry-dir <dir>] [--out <file>]

OPTIONS:
    --telemetry-dir <dir>   Telemetry root to read (expects a `sessions/`
                            subdirectory). Defaults to this platform's
                            telemetry directory; falls back to it when the
                            given directory holds no ledger.
    --out <file>            Write the JSON report here (also printed to stdout).
    -h, --help              Show this help";

// ─────────────────────────────────────────────────────────────────────────────
// Session model
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct Session {
    start_ms: Option<i64>,
    end_ms: Option<i64>,
    reason: Option<String>,
    commit: Option<String>,
    version: Option<String>,
    os: Option<String>,
    arch: Option<String>,
    surface: Option<String>,
    start_us: Option<u64>,
}

fn main() {
    let mut requested: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--telemetry-dir" => {
                i += 1;
                match args.get(i) {
                    Some(v) => requested = Some(PathBuf::from(v)),
                    None => fail("--telemetry-dir needs a value"),
                }
            }
            "--out" => {
                i += 1;
                match args.get(i) {
                    Some(v) => out = Some(PathBuf::from(v)),
                    None => fail("--out needs a value"),
                }
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            other => fail(&format!("unknown argument {other}")),
        }
        i += 1;
    }

    let platform = eustress_engine::usage_telemetry::telemetry_dir();
    let (used, ledger_files) = resolve_dir(requested.as_deref(), platform.as_deref());

    let mut sessions: BTreeMap<String, Session> = BTreeMap::new();
    let mut malformed_lines = 0usize;
    for file in &ledger_files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                malformed_lines += 1;
                continue;
            };
            let Some(sid) = v.get("sid").and_then(|s| s.as_str()) else {
                malformed_lines += 1;
                continue;
            };
            let entry = sessions.entry(sid.to_string()).or_default();
            match v.get("rec").and_then(|r| r.as_str()) {
                Some("start") => {
                    entry.start_ms = v.get("ts").and_then(|t| t.as_i64());
                    entry.commit = str_field(&v, "commit");
                    entry.version = str_field(&v, "ver");
                    entry.os = str_field(&v, "os");
                    entry.arch = str_field(&v, "arch");
                    entry.surface = str_field(&v, "surface");
                }
                Some("end") => {
                    entry.end_ms = v.get("ts").and_then(|t| t.as_i64());
                    entry.reason = str_field(&v, "reason");
                    entry.start_us = v.get("start_us").and_then(|t| t.as_u64());
                }
                _ => malformed_lines += 1,
            }
        }
    }

    // An `end` with no `start` is not a session — it is a fragment of one
    // whose start predates the ledger. Counting it would inflate the
    // denominator with something that was never observed starting.
    let orphan_ends = sessions.values().filter(|s| s.start_ms.is_none()).count();
    sessions.retain(|_, s| s.start_ms.is_some());

    let mut reason_hist: BTreeMap<String, u64> = BTreeMap::new();
    let mut commit_hist: BTreeMap<String, u64> = BTreeMap::new();
    let mut version_hist: BTreeMap<String, u64> = BTreeMap::new();
    let mut os_hist: BTreeMap<String, u64> = BTreeMap::new();
    let mut surface_hist: BTreeMap<String, u64> = BTreeMap::new();
    let mut unclean = 0u64;
    let mut unreconciled = 0u64;
    let mut first_ms = i64::MAX;
    let mut last_ms = i64::MIN;
    let mut max_start_us = 0u64;
    let mut sum_start_us = 0u64;
    let mut n_start_us = 0u64;

    for s in sessions.values() {
        let start = s.start_ms.unwrap_or_default();
        first_ms = first_ms.min(start);
        last_ms = last_ms.max(s.end_ms.unwrap_or(start));

        // No end record yet: the process died without writing one and no
        // later launch has reconciled it. Same outcome, same reason string.
        let reason = match &s.reason {
            Some(r) => r.clone(),
            None => {
                unreconciled += 1;
                EndReason::Orphaned.as_str().to_string()
            }
        };
        if !EndReason::wire_is_clean(&reason) {
            unclean += 1;
        }
        *reason_hist.entry(reason).or_insert(0) += 1;
        *commit_hist
            .entry(s.commit.clone().unwrap_or_else(|| "unknown".into()))
            .or_insert(0) += 1;
        *version_hist
            .entry(s.version.clone().unwrap_or_else(|| "unknown".into()))
            .or_insert(0) += 1;
        *os_hist
            .entry(format!(
                "{}/{}",
                s.os.clone().unwrap_or_else(|| "unknown".into()),
                s.arch.clone().unwrap_or_else(|| "unknown".into())
            ))
            .or_insert(0) += 1;
        *surface_hist
            .entry(s.surface.clone().unwrap_or_else(|| "unknown".into()))
            .or_insert(0) += 1;
        if let Some(us) = s.start_us {
            max_start_us = max_start_us.max(us);
            sum_start_us += us;
            n_start_us += 1;
        }
    }

    let total = sessions.len() as u64;
    let rate = if total == 0 {
        0.0
    } else {
        1.0 - (unclean as f64) / (total as f64)
    };

    // Precomputed so the report literal below stays a flat description of the
    // finding rather than a place where arithmetic hides.
    let clean_exits = reason_hist.get("clean").copied().unwrap_or(0);
    let induced_panics = reason_hist.get("panic").copied().unwrap_or(0);
    let hard_deaths = reason_hist.get("orphaned").copied().unwrap_or(0);
    let first_iso: serde_json::Value = if total == 0 {
        serde_json::Value::Null
    } else {
        iso_utc(first_ms).into()
    };
    let last_iso: serde_json::Value = if total == 0 {
        serde_json::Value::Null
    } else {
        iso_utc(last_ms).into()
    };
    let mean_start_us = if n_start_us == 0 { 0 } else { sum_start_us / n_start_us };

    let report = serde_json::json!({
        "item": "G7.30",
        "what": "Crash-free-session baseline computed from the local session ledger.",
        "generated_at": iso_utc(now_millis()),
        "telemetry_dir_requested": requested.as_ref().map(path_str),
        "telemetry_dir_used": used.as_ref().map(|p| path_str(p)),
        "ledger_files_read": ledger_files.len(),

        "sessions_total": total,
        "sessions_unclean": unclean,
        "crash_free_rate": rate,

        "end_reason_histogram": reason_hist,
        "sessions_unreconciled_at_read": unreconciled,
        "end_records_without_start_ignored": orphan_ends,
        "malformed_lines_ignored": malformed_lines,

        "date_range_utc": {
            "first_session_start": first_iso,
            "last_session_end": last_iso,
        },
        "engine_commit_histogram": commit_hist,
        "app_version_histogram": version_hist,
        "os_histogram": os_hist,
        "surface_histogram": surface_hist,

        // The mix is derived, never typed in: a hard kill leaves an orphan and
        // an induced panic leaves `panic`, so the histogram IS the proof the
        // detector fired. A hard kill is, by construction, indistinguishable
        // from any other death that writes nothing — that honesty is the
        // design, not a gap in it.
        "detector_proof": {
            "clean_exits": clean_exits,
            "induced_panics_recorded": induced_panics,
            "hard_deaths_reconciled_as_orphans": hard_deaths,
        },

        "beacon_startup_cost_us": {
            "sessions_measured": n_start_us,
            "max": max_start_us,
            "mean": mean_start_us,
            "budget": 20_000,
            "note": "Microseconds spent in begin_session (ledger scan + orphan reconcile + fsynced append). Budget is the item's 20 ms startup ceiling.",
        },
    });

    let text = serde_json::to_string_pretty(&report).unwrap_or_else(|e| {
        fail(&format!("serialize failed: {e}"));
        unreachable!()
    });
    println!("{text}");

    if let Some(path) = out {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&path, format!("{text}\n")) {
            fail(&format!("cannot write {}: {e}", path.display()));
        }
        eprintln!("session-baseline: wrote {}", path.display());
    }

    eprintln!(
        "session-baseline: total={total} unclean={unclean} crash_free_rate={rate:.6}"
    );
}

/// Pick the directory to read. The requested one wins when it actually holds a
/// ledger; otherwise this falls back to the platform telemetry directory and
/// says so. Path resolution only — no session is ever included or excluded by
/// this choice, and both paths land in the report.
fn resolve_dir(requested: Option<&Path>, platform: Option<&Path>) -> (Option<PathBuf>, Vec<PathBuf>) {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(r) = requested {
        candidates.push(r.to_path_buf());
    }
    if let Some(p) = platform {
        if !candidates.iter().any(|c| c == p) {
            candidates.push(p.to_path_buf());
        }
    }
    for c in &candidates {
        let files = ledger_files(c);
        if !files.is_empty() {
            if Some(c.as_path()) != requested {
                eprintln!(
                    "session-baseline: no ledger under {:?}; reading {:?} instead",
                    requested.map(|r| r.display().to_string()).unwrap_or_default(),
                    c.display()
                );
            }
            return (Some(c.clone()), files);
        }
    }
    (candidates.into_iter().next(), Vec::new())
}

/// Every `*.jsonl` in `<dir>/sessions`, so splitting the ledger by month later
/// needs no change here. The canonical name is checked first for determinism.
fn ledger_files(root: &Path) -> Vec<PathBuf> {
    let dir = root.join("sessions");
    let mut files: Vec<PathBuf> = Vec::new();
    let canonical = dir.join(SESSION_LEDGER_FILE);
    if canonical.is_file() {
        files.push(canonical.clone());
    }
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut rest: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
            .filter(|p| *p != canonical)
            .collect();
        rest.sort();
        files.append(&mut rest);
    }
    files
}

fn str_field(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|s| s.as_str()).map(|s| s.to_string())
}

fn path_str(p: &PathBuf) -> String {
    p.display().to_string()
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// `YYYY-MM-DDTHH:MM:SSZ` from unix millis. Civil-from-days (Hinnant), same
/// arithmetic the telemetry module uses for its daily file names — one less
/// dependency for one date string.
fn iso_utc(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

fn fail(msg: &str) -> ! {
    eprintln!("session-baseline: {msg}\n\n{USAGE}");
    std::process::exit(2);
}
