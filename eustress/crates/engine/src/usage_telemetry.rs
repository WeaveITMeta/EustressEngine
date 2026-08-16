//! Usage telemetry — which discipline tools people actually reach for.
//!
//! Eustress ships ~1,400 ribbon tools of which only ~30 are wired; the rest
//! are deliberate "dream" buttons (see `tool_metadata.rs`). Launching that way
//! is only defensible if two things are true: clicking a dream button tells
//! the user the truth, and the click is counted so the wiring backlog is
//! ranked by real demand instead of guesswork. This module does both.
//!
//! # Privacy posture (deliberate)
//! * **Anonymous.** A random install UUID, rotatable by deleting one file.
//!   Never tied to the Bliss/KYC account — usage interest and identity stay
//!   separate on purpose.
//! * **No content.** Click telemetry carries only the tool id, the
//!   mode/discipline it was clicked in, wiredness, and a timestamp — never
//!   scene data, file paths, or entity names. The only free text that ever
//!   leaves the machine is what a user explicitly types into Help ▸ Send
//!   Feedback and presses Send on ([`post_comment`]).
//! * **Inspectable.** Events land as plain JSONL under the user's own
//!   `Eustress/telemetry/` folder — "what we collect" is readable with a text
//!   editor, not a promise in a policy.
//! * **On by default, one toggle off.** Settings ▸ Notifications ▸ Privacy;
//!   a first-run toast says so plainly. Disabling stops capture at the source
//!   (no buffering, no file writes).
//!
//! # Upload (aggregates only)
//! At session end the per-tool **totals** (never the per-click event stream)
//! are written to an `outbox/` file and posted to
//! `https://api.eustress.dev/api/telemetry/usage` on a background thread; a
//! failed or interrupted post is retried at next launch by draining the
//! outbox. Per-click timing therefore never leaves the machine. Override the
//! endpoint with `EUSTRESS_TELEMETRY_URL` (e.g. for a local worker dev run).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::io::Write;

/// One ribbon-tool click. Field names are short because this is appended as
/// JSONL and read back by aggregate tooling, not by humans (though it stays
/// human-readable on purpose).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ClickEvent {
    /// Unix epoch milliseconds (UTC).
    pub ts: i64,
    /// The manifest tool id, e.g. `"math:equation_solver"`.
    pub tool: String,
    /// Active mode id, e.g. `"student"`.
    pub mode: String,
    /// Active submode/discipline id, e.g. `"mathematics"` (empty if none).
    pub disc: String,
    /// True when the click actually did something; false for a dream button.
    pub wired: bool,
}

/// Buffered, opt-out-able recorder for ribbon-tool clicks.
///
/// Clicks accumulate in memory and are appended to a daily JSONL file by
/// [`flush_usage_telemetry`] — batching keeps a fast click-through from
/// turning into one file write per button.
#[derive(Resource)]
pub struct UsageTelemetry {
    /// Mirrors `EditorSettings.usage_telemetry_enabled`. When false nothing
    /// is recorded, buffered, or written.
    pub enabled: bool,
    /// Random per-install id. Not an account id and never joined to one.
    pub install_id: String,
    /// Pending events awaiting the next flush.
    buffer: Vec<ClickEvent>,
    /// Seconds since the last flush (batching timer).
    since_flush: f32,
    /// True once the first-run privacy notice has been shown this session.
    pub notice_shown: bool,
    /// Session-cumulative clicks per tool id — survives buffer flushes and
    /// becomes the ONE aggregate uploaded at session end. Aggregates (not
    /// events) are what cross the network, so per-click timing never leaves
    /// the machine.
    session_counts: std::collections::HashMap<String, u64>,
    /// Mode active for the plurality of recorded clicks (simple last-writer;
    /// good enough for a per-session context tag).
    session_mode: String,
    /// Last unwired tool clicked this session — pre-fills the Help ▸ Send
    /// Feedback dialog with the thing the user most recently wished worked.
    pub last_dream_tool: String,
}

impl Default for UsageTelemetry {
    fn default() -> Self {
        Self {
            enabled: true,
            install_id: load_or_create_install_id(),
            buffer: Vec::new(),
            since_flush: 0.0,
            notice_shown: false,
            session_counts: std::collections::HashMap::new(),
            session_mode: String::new(),
            last_dream_tool: String::new(),
        }
    }
}

impl UsageTelemetry {
    /// Record a click. No-op when disabled — the opt-out is enforced at the
    /// source, so a disabled install never even buffers.
    pub fn record(&mut self, tool: &str, mode: &str, disc: &str, wired: bool) {
        if !self.enabled {
            return;
        }
        self.buffer.push(ClickEvent {
            ts: now_millis(),
            tool: tool.to_string(),
            mode: mode.to_string(),
            disc: disc.to_string(),
            wired,
        });
        *self.session_counts.entry(tool.to_string()).or_insert(0) += 1;
        self.session_mode = mode.to_string();
        if !wired {
            self.last_dream_tool = tool.to_string();
        }
        // Hard bound: a runaway click loop can never eat memory. Dropping the
        // oldest is right here — recency is what the demand ranking wants.
        const MAX_BUFFER: usize = 4096;
        if self.buffer.len() > MAX_BUFFER {
            let overflow = self.buffer.len() - MAX_BUFFER;
            self.buffer.drain(..overflow);
        }
    }

    /// Events waiting to be written (used by tests and the flush system).
    pub fn pending(&self) -> usize {
        self.buffer.len()
    }

    /// Drop everything buffered without writing it. Called when the user
    /// opts out mid-session so their decision covers clicks already made in
    /// this session, not just future ones.
    pub fn discard_buffered(&mut self) {
        self.buffer.clear();
        self.session_counts.clear();
        // One toggle, one effect: the session ledger loses this session too,
        // so opting out cannot leave half the pipeline still recording.
        purge_session_records();
    }

    /// Append the buffer to today's JSONL file and clear it. Returns how many
    /// events were written. Failure is non-fatal and never blocks the editor —
    /// telemetry must not be able to break someone's session.
    pub fn flush(&mut self) -> usize {
        if self.buffer.is_empty() {
            return 0;
        }
        let dir = match telemetry_dir() {
            Some(d) => d,
            None => {
                self.buffer.clear();
                return 0;
            }
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            warn!("usage telemetry: cannot create {:?}: {e}", dir);
            self.buffer.clear();
            return 0;
        }
        let path = dir.join(format!("usage-{}.jsonl", today_stamp()));
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&path);
        let mut file = match file {
            Ok(f) => f,
            Err(e) => {
                warn!("usage telemetry: cannot open {:?}: {e}", path);
                self.buffer.clear();
                return 0;
            }
        };
        let mut written = 0;
        for ev in self.buffer.drain(..) {
            match serde_json::to_string(&ev) {
                Ok(line) => {
                    if writeln!(file, "{line}").is_ok() {
                        written += 1;
                    }
                }
                Err(e) => warn!("usage telemetry: serialize failed: {e}"),
            }
        }
        written
    }
}

/// `%LOCALAPPDATA%/Eustress/telemetry` (or the platform equivalent).
pub fn telemetry_dir() -> Option<std::path::PathBuf> {
    dirs::data_local_dir().map(|d| d.join("Eustress").join("telemetry"))
}

// ─────────────────────────────────────────────────────────────────────────────
// Session lifecycle beacon
// ─────────────────────────────────────────────────────────────────────────────
//
// The click pipeline above answers "which tools do people reach for". The
// beacon below answers a different question the same rail can carry: **did
// this session end the way it was supposed to?**
//
// A session that dies hard — a kill, a driver reset, a power loss — writes
// nothing at exit, so without a *start* record it leaves no trace at all and
// is indistinguishable from a session that never happened. That makes the
// denominator of "crash-free sessions" unknowable, not merely imprecise.
//
// The fix is deliberately small and has exactly two disk touches per session:
//
//   1. **At construction time**, before any Space begins to load, append one
//      `start` line to an append-only ledger and `sync_data` it. A crash
//      during load — the class the load-phase watchdog exists for — is
//      therefore already covered by the time loading begins.
//   2. **At the end**, append one `end` line carrying a closed-enum reason.
//      A clean `AppExit` writes `clean`; the panic hook writes `panic`.
//
// Anything else (SIGKILL, `std::process::exit` off the panic path, power
// loss) leaves the `start` unmatched. Nothing can be written in those cases,
// so nothing pretends to be: the **next** launch scans the ledger, finds
// starts with no end, and appends `orphaned` end records for them. Honest
// reconciliation on the next launch beats a signal handler that claims to
// catch a case it cannot.
//
// # Privacy
// A session record carries the install id already used by the click pipeline,
// a random per-session id, timestamps, the end reason, the app version, the
// engine commit, `std::env::consts` OS/arch, a closed-set surface tag, and a
// monotonic sequence number. It carries **no** Space name, no universe name,
// no file path, and no free text — grep the ledger for a path separator and
// you will not find one. The single Settings ▸ Notifications ▸ Privacy toggle
// governs the beacon and the click pipeline together: with it off no start
// record is written at all, and turning it off mid-session erases this
// session's records from the ledger (see [`UsageTelemetry::discard_buffered`]).

/// How a session ended. This is the **one** closed vocabulary for session
/// outcomes in the engine; nothing else may invent a parallel one.
///
/// Every variant except [`EndReason::Clean`] counts as an unclean session, and
/// the crash-free rate is `1 - unclean / total` over the local ledger.
///
/// # Extension contract (for later work)
///
/// This enum is expected to grow. Extend it — never fork it:
///
/// * Add a variant here and give it a `as_str()` wire string that is **stable
///   forever**. The wire string is what lands in the on-disk ledger, and every
///   historical artifact is keyed by it; renaming one silently rewrites past
///   measurements.
/// * Decide [`EndReason::is_clean`] for the new variant explicitly. Default to
///   unclean: a reason nobody has classified is not evidence of a good exit.
/// * Do **not** add an `Unknown` / catch-all variant. An unrecognised wire
///   string is already handled — [`EndReason::wire_is_clean`] answers `false`
///   for it, so an old reader meeting a new engine's reason counts the session
///   as unclean and passes the string through to the histogram rather than
///   dropping it.
///
/// Reserved wire strings, so the follow-on items do not collide:
/// `gpu_surface_lost`, `render_device_lost` (panic classification);
/// `smoke_timeout`, `smoke_abort` (CI smoke-test accept codes);
/// `shutdown_timeout`, `restart` (clean-exit contract).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    /// Bevy delivered `AppExit` and the shutdown path ran to completion.
    Clean,
    /// The process panicked. The panic hook wrote this before unwinding, so it
    /// is recorded even when a later `catch_unwind` converts the panic into a
    /// zero exit code (which `main.rs` does today for suspected GPU
    /// surface-loss). Splitting this into specific fault classes is exactly
    /// what the extension contract above is for.
    Panic,
    /// A `start` record with no matching `end`, discovered by a **later**
    /// launch. The process died without the chance to write anything: a hard
    /// kill, a power loss, an abort, or an exit that bypassed `AppExit`.
    Orphaned,
}

impl EndReason {
    /// Stable on-disk wire string. Never change an existing mapping.
    pub const fn as_str(self) -> &'static str {
        match self {
            EndReason::Clean => "clean",
            EndReason::Panic => "panic",
            EndReason::Orphaned => "orphaned",
        }
    }

    /// Only `Clean` is clean. Everything else — known or not — is not.
    pub const fn is_clean(self) -> bool {
        matches!(self, EndReason::Clean)
    }

    /// Parse a wire string written by this or an older engine. Returns `None`
    /// for a reason this build does not know about (a newer engine's).
    pub fn from_wire(s: &str) -> Option<Self> {
        match s {
            "clean" => Some(EndReason::Clean),
            "panic" => Some(EndReason::Panic),
            "orphaned" => Some(EndReason::Orphaned),
            _ => None,
        }
    }

    /// Forward-compatible cleanliness test for a raw ledger string. An
    /// unrecognised reason is **not** clean, so a reader can never turn a
    /// future fault class into a passing session by not knowing about it.
    pub fn wire_is_clean(s: &str) -> bool {
        Self::from_wire(s).map(Self::is_clean).unwrap_or(false)
    }
}

/// Live-session handle. Held in a `static` rather than a Bevy resource
/// because the panic hook runs with no access to the `World`.
struct SessionBeacon {
    /// Random per-session id (not the install id, not an account id).
    sid: String,
    /// Full path to the append-only ledger file.
    ledger: std::path::PathBuf,
    /// Session start, unix epoch millis.
    start_ms: i64,
    /// Microseconds the whole start step cost (ledger scan + orphan
    /// reconcile + durable append). Recorded in the end record so the
    /// startup-cost budget is measured from real sessions, not asserted.
    start_us: u64,
}

static BEACON: std::sync::OnceLock<SessionBeacon> = std::sync::OnceLock::new();
static SESSION_ENDED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `<telemetry>/sessions` — the beacon's own subdirectory, so the daily click
/// JSONL and the session ledger never fight over a file name.
pub fn session_ledger_dir() -> Option<std::path::PathBuf> {
    telemetry_dir().map(|d| d.join("sessions"))
}

/// Append-only ledger file name. Readers glob `*.jsonl` in the directory, so
/// splitting this by month later needs no reader change.
pub const SESSION_LEDGER_FILE: &str = "sessions.jsonl";

/// Which surface the session ran on. A closed set derived from the binary's
/// own file stem — never the path, so no path separator can reach the ledger.
fn surface_kind() -> &'static str {
    let stem = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()))
        .unwrap_or_default();
    if stem.contains("headless") {
        "headless"
    } else if stem.contains("client") {
        "client"
    } else if stem.contains("server") {
        "server"
    } else {
        "editor"
    }
}

/// Engine commit the binary was built from. Supplied at compile time by the
/// build environment (`EUSTRESS_GIT_SHA`); `unknown` when a build did not set
/// it, which is itself worth seeing in the histogram.
fn engine_commit() -> &'static str {
    option_env!("EUSTRESS_GIT_SHA").unwrap_or("unknown")
}

/// Append `data` and make it durable before returning. One `sync_data` per
/// call; the beacon calls this at most twice in a session's life.
fn append_durable(path: &std::path::Path, data: &str) -> bool {
    let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) else {
        return false;
    };
    if f.write_all(data.as_bytes()).is_err() {
        return false;
    }
    let _ = f.flush();
    f.sync_data().is_ok()
}

/// Pure ledger analysis, split out so it is testable without a filesystem.
///
/// Returns the session ids that started and never ended (in first-seen order)
/// and the sequence number the next session should take.
fn scan_ledger_text(text: &str) -> (Vec<String>, u64) {
    let mut started: Vec<String> = Vec::new();
    let mut ended: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut max_seq: u64 = 0;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(sid) = v.get("sid").and_then(|s| s.as_str()) else {
            continue;
        };
        match v.get("rec").and_then(|r| r.as_str()) {
            Some("start") => {
                started.push(sid.to_string());
                if let Some(seq) = v.get("seq").and_then(|s| s.as_u64()) {
                    max_seq = max_seq.max(seq);
                }
            }
            Some("end") => {
                ended.insert(sid.to_string());
            }
            _ => {}
        }
    }
    let orphans = started.into_iter().filter(|s| !ended.contains(s)).collect();
    (orphans, max_seq + 1)
}

/// Open the session: reconcile any orphans left by earlier launches, then
/// append this session's durable `start` record.
///
/// Call this at plugin-construction time — before a Space can begin loading —
/// and exactly once per process. Returns false when telemetry has no writable
/// home, in which case the whole beacon stays silent.
pub fn begin_session(install_id: &str) -> bool {
    if BEACON.get().is_some() {
        return true;
    }
    let t0 = std::time::Instant::now();
    let Some(dir) = session_ledger_dir() else {
        return false;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let ledger = dir.join(SESSION_LEDGER_FILE);

    let existing = std::fs::read_to_string(&ledger).unwrap_or_default();
    let (orphans, seq) = scan_ledger_text(&existing);

    let sid = uuid::Uuid::new_v4().to_string();
    let start_ms = now_millis();

    // One buffer, one append, one fsync — orphan reconciliation and the start
    // record share the single write this session is allowed at startup.
    let mut out = String::new();
    for orphan in &orphans {
        out.push_str(
            &serde_json::json!({
                "rec": "end",
                "sid": orphan,
                "ts": start_ms,
                "reason": EndReason::Orphaned.as_str(),
                "reconciled": true,
            })
            .to_string(),
        );
        out.push('\n');
    }
    out.push_str(
        &serde_json::json!({
            "rec": "start",
            "sid": sid,
            "install": install_id,
            "seq": seq,
            "ts": start_ms,
            "ver": env!("CARGO_PKG_VERSION"),
            "commit": engine_commit(),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "surface": surface_kind(),
        })
        .to_string(),
    );
    out.push('\n');

    if !append_durable(&ledger, &out) {
        return false;
    }
    if !orphans.is_empty() {
        // Deliberately printed, not `info!`: this runs before the log plugin
        // has a subscriber, and "the last N sessions died hard" is worth
        // seeing in a terminal.
        println!(
            "ℹ session beacon: reconciled {} unclean session(s) from earlier launches",
            orphans.len()
        );
    }
    let _ = BEACON.set(SessionBeacon {
        sid,
        ledger,
        start_ms,
        start_us: t0.elapsed().as_micros() as u64,
    });
    true
}

/// Close the session with a reason. Idempotent and safe from a panic hook:
/// the first caller wins, so a panic followed by a shutdown path (or the
/// reverse) records one end record, not two.
pub fn end_session(reason: EndReason) {
    let Some(b) = BEACON.get() else {
        return;
    };
    if SESSION_ENDED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let ts = now_millis();
    let line = serde_json::json!({
        "rec": "end",
        "sid": b.sid,
        "ts": ts,
        "reason": reason.as_str(),
        "dur_ms": ts - b.start_ms,
        "start_us": b.start_us,
    })
    .to_string();
    append_durable(&b.ledger, &format!("{line}\n"));
}

/// Erase this session from the ledger. Called when the user opts out
/// mid-session so the single privacy toggle covers the beacon and the click
/// buffer with one click — not one of the two.
pub fn purge_session_records() {
    let Some(b) = BEACON.get() else {
        return;
    };
    SESSION_ENDED.store(true, std::sync::atomic::Ordering::SeqCst);
    let Ok(text) = std::fs::read_to_string(&b.ledger) else {
        return;
    };
    let kept: String = text
        .lines()
        .filter(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|v| v.get("sid").and_then(|s| s.as_str()).map(|s| s != b.sid))
                .unwrap_or(true)
        })
        .map(|l| format!("{l}\n"))
        .collect();
    let _ = std::fs::write(&b.ledger, kept);
}

/// Record a panic as the session's end reason before the unwind continues.
///
/// The hook is chained, not replaced, so the normal panic message and any
/// previously installed hook still run. It fires even on the paths that later
/// swallow the panic — `main.rs` converts a suspected GPU surface loss into
/// `std::process::exit(0)`, which never reaches `AppExit`, and this hook is
/// the only reason that session is not lost.
fn install_panic_beacon() {
    static INSTALLED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if INSTALLED.set(()).is_err() {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        end_session(EndReason::Panic);
        previous(info);
    }));
}

/// Fault injection: panic on purpose so the beacon's crash path can be
/// **proven** to fire rather than merely to compile. No-op unless
/// `EUSTRESS_SESSION_BEACON_PANIC_AT_FRAME` is set to a frame count, which no
/// shipped configuration does. An untested detector is not a detector.
fn beacon_panic_probe(mut frames: Local<u32>) {
    let Some(at) = std::env::var("EUSTRESS_SESSION_BEACON_PANIC_AT_FRAME")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
    else {
        return;
    };
    *frames += 1;
    if *frames >= at {
        panic!("session beacon fault injection: deliberate panic at frame {}", *frames);
    }
}

/// Ingest endpoint — the production api worker unless overridden for dev.
fn upload_url() -> String {
    std::env::var("EUSTRESS_TELEMETRY_URL")
        .unwrap_or_else(|_| "https://api.eustress.dev/api/telemetry/usage".to_string())
}

impl UsageTelemetry {
    /// Serialize this session's aggregate into the outbox. Called at exit;
    /// the actual network send happens via [`drain_outbox`] (either the
    /// best-effort thread spawned right after, or next launch's drain).
    /// Writing BEFORE sending is the crash/offline guarantee.
    pub fn write_outbox(&mut self) {
        if !self.enabled || self.session_counts.is_empty() {
            return;
        }
        let Some(dir) = telemetry_dir() else { return };
        let outbox = dir.join("outbox");
        if std::fs::create_dir_all(&outbox).is_err() {
            return;
        }
        let payload = serde_json::json!({
            "install_id": self.install_id,
            "app_version": env!("CARGO_PKG_VERSION"),
            "mode": self.session_mode,
            "counts": self.session_counts,
        });
        let path = outbox.join(format!("session-{}.json", now_millis()));
        if let Err(e) = std::fs::write(&path, payload.to_string()) {
            warn!("usage telemetry: outbox write failed: {e}");
        }
        self.session_counts.clear();
    }
}

/// Fire-and-forget comment post (Help ▸ Send Feedback). Runs on its own
/// thread; a lost comment on network failure is acceptable — unlike counts,
/// feedback text is explicitly typed and the user sees a "sent" toast only
/// after we spawn, so no false promises are made about delivery guarantees.
pub fn post_comment(install_id: String, tool: String, text: String) {
    std::thread::spawn(move || {
        let url = std::env::var("EUSTRESS_TELEMETRY_COMMENT_URL")
            .unwrap_or_else(|_| "https://api.eustress.dev/api/telemetry/comment".to_string());
        let payload = serde_json::json!({
            "install_id": install_id,
            "tool": tool,
            "text": text,
        });
        let _ = ureq::post(&url)
            .timeout(std::time::Duration::from_secs(5))
            .set("Content-Type", "application/json")
            .send_string(&payload.to_string());
    });
}

/// Post every pending outbox file, deleting each on success. Runs on a
/// plain thread — telemetry may never block the editor or its shutdown.
/// Failures are left in place for the next attempt; a file that is somehow
/// unreadable/corrupt is removed rather than retried forever.
pub fn drain_outbox() {
    let Some(dir) = telemetry_dir() else { return };
    let outbox = dir.join("outbox");
    let Ok(entries) = std::fs::read_dir(&outbox) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&path) else {
            let _ = std::fs::remove_file(&path);
            continue;
        };
        if serde_json::from_str::<serde_json::Value>(&body).is_err() {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        let ok = ureq::post(&upload_url())
            .timeout(std::time::Duration::from_secs(5))
            .set("Content-Type", "application/json")
            .send_string(&body)
            .map(|r| (200..300).contains(&r.status()))
            .unwrap_or(false);
        if ok {
            let _ = std::fs::remove_file(&path);
        }
        // Not-ok: leave the file for the next launch's drain.
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// `YYYYMMDD` in UTC, for the daily file name.
fn today_stamp() -> String {
    let secs = now_millis() / 1000;
    // Civil-from-days (Howard Hinnant's algorithm) — avoids pulling a date
    // crate in for one filename.
    let days = secs.div_euclid(86_400);
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
    format!("{y:04}{m:02}{d:02}")
}

/// Read the persisted install id, creating one on first run. Deleting the
/// file rotates the identity — that is the documented reset mechanism.
fn load_or_create_install_id() -> String {
    let Some(dir) = telemetry_dir() else {
        return "unknown".to_string();
    };
    let path = dir.join("install-id.txt");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(&path, &id);
    id
}

/// Batches writes: flush every `FLUSH_INTERVAL_SECS`, so a burst of clicks
/// costs one append rather than one per button.
const FLUSH_INTERVAL_SECS: f32 = 30.0;

fn flush_usage_telemetry(
    mut telemetry: ResMut<UsageTelemetry>,
    time: Res<Time>,
) {
    if telemetry.pending() == 0 {
        return;
    }
    telemetry.since_flush += time.delta_secs();
    if telemetry.since_flush >= FLUSH_INTERVAL_SECS {
        telemetry.since_flush = 0.0;
        telemetry.flush();
    }
}

/// Final flush + upload handoff on quit: JSONL to disk, session aggregate to
/// the outbox, then a best-effort background send. If the process dies before
/// the thread finishes, the outbox file survives and next launch's
/// [`startup_drain`] delivers it — nothing is lost, nothing blocks shutdown.
fn flush_on_exit(
    mut exits: MessageReader<AppExit>,
    mut telemetry: ResMut<UsageTelemetry>,
) {
    if exits.read().next().is_some() {
        let n = telemetry.flush();
        if n > 0 {
            info!("usage telemetry: flushed {n} events on exit");
        }
        telemetry.write_outbox();
        // The session ends clean only here — the one path that observes a
        // real `AppExit`. Everything else is a panic (hook) or an orphan
        // (reconciled by the next launch).
        end_session(EndReason::Clean);
        std::thread::spawn(drain_outbox);
    }
}

/// One-shot startup system: retry any outbox files a previous session failed
/// to deliver (offline, crash, or exit racing the send thread).
fn startup_drain(mut done: Local<bool>, telemetry: Res<UsageTelemetry>) {
    if *done {
        return;
    }
    *done = true;
    if telemetry.enabled {
        std::thread::spawn(drain_outbox);
    }
}

/// Sync the runtime flag from persisted settings, and show the first-run
/// privacy notice exactly once per install. The notice is a real toast (not a
/// buried policy line) because "on by default" is only honest if it is said
/// plainly, up front, with the off-switch named.
fn sync_and_notice(
    mut telemetry: ResMut<UsageTelemetry>,
    mut settings: Option<ResMut<crate::editor_settings::EditorSettings>>,
    mut notify: MessageWriter<crate::ui::notifications::NotificationEvent>,
) {
    let Some(settings) = settings.as_mut() else {
        return;
    };
    // Settings is the source of truth for the toggle.
    if telemetry.enabled != settings.usage_telemetry_enabled {
        telemetry.enabled = settings.usage_telemetry_enabled;
    }
    if telemetry.notice_shown || settings.telemetry_notice_shown {
        telemetry.notice_shown = true;
        return;
    }
    telemetry.notice_shown = true;
    settings.telemetry_notice_shown = true;
    let _ = settings.save();
    notify.write(crate::ui::notifications::NotificationEvent::info(
        crate::ui::notifications::NotificationCategory::General,
        "Anonymous usage stats are on",
        // Plain words, no separator glyph. The toast font has no `▸`
        // (U+25B8), so "Settings ▸ Notifications ▸ Privacy" rendered as
        // "Settings Notifications Privacy" — three words with nothing to say
        // one contains the next, which reads as a broken path rather than a
        // route. Naming the tab and the section outright also survives any
        // font, and matches what the Settings dialog actually shows.
        "Eustress counts which tools you click (never your content) to decide what to build next. \
         Turn it off any time in Settings, on the Notifications tab under Privacy.",
    ));
}

pub struct UsageTelemetryPlugin;

impl Plugin for UsageTelemetryPlugin {
    fn build(&self, app: &mut App) {
        // Open the session HERE, during app construction, and not from a
        // startup system: a Space begins loading inside the schedule, so a
        // start record written by a system cannot observe a crash during load
        // — the very class the load-phase watchdog exists for. The settings
        // file is the source of truth for the opt-out, and it is read
        // directly because the `EditorSettings` resource is not guaranteed to
        // exist yet at this point in plugin construction.
        if crate::editor_settings::EditorSettings::load().usage_telemetry_enabled {
            begin_session(&load_or_create_install_id());
            install_panic_beacon();
        }

        app.init_resource::<UsageTelemetry>()
            .add_systems(
                Update,
                (
                    flush_usage_telemetry,
                    flush_on_exit,
                    sync_and_notice,
                    startup_drain,
                    beacon_panic_probe,
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_records_nothing() {
        let mut t = UsageTelemetry {
            enabled: false,
            install_id: "test".into(),
            buffer: Vec::new(),
            since_flush: 0.0,
            notice_shown: true,
            session_counts: std::collections::HashMap::new(),
            session_mode: String::new(),
            last_dream_tool: String::new(),
        };
        t.record("math:equation_solver", "student", "mathematics", false);
        assert_eq!(t.pending(), 0, "opt-out must stop capture at the source");
        assert!(t.session_counts.is_empty(), "opt-out must not aggregate either");
    }

    #[test]
    fn enabled_buffers_and_is_bounded() {
        let mut t = UsageTelemetry {
            enabled: true,
            install_id: "test".into(),
            buffer: Vec::new(),
            since_flush: 0.0,
            notice_shown: true,
            session_counts: std::collections::HashMap::new(),
            session_mode: String::new(),
            last_dream_tool: String::new(),
        };
        t.record("math:equation_solver", "student", "mathematics", false);
        assert_eq!(t.pending(), 1);
        assert_eq!(t.last_dream_tool, "math:equation_solver");
        assert_eq!(t.session_counts.get("math:equation_solver"), Some(&1));
        for _ in 0..5000 {
            t.record("x:y", "m", "d", true);
        }
        assert!(t.pending() <= 4096, "buffer must stay bounded");
    }

    #[test]
    fn end_reason_wire_strings_are_stable() {
        // These three strings are on disk in every historical ledger. If this
        // test ever needs changing, past measurements are being rewritten.
        assert_eq!(EndReason::Clean.as_str(), "clean");
        assert_eq!(EndReason::Panic.as_str(), "panic");
        assert_eq!(EndReason::Orphaned.as_str(), "orphaned");
        assert_eq!(EndReason::from_wire("panic"), Some(EndReason::Panic));
        assert_eq!(EndReason::from_wire("gpu_surface_lost"), None);
    }

    #[test]
    fn only_clean_counts_as_clean_and_unknown_never_does() {
        assert!(EndReason::wire_is_clean("clean"));
        assert!(!EndReason::wire_is_clean("panic"));
        assert!(!EndReason::wire_is_clean("orphaned"));
        // A reason from a newer engine must fall on the unclean side, never
        // pass by default.
        assert!(!EndReason::wire_is_clean("smoke_timeout"));
        assert!(!EndReason::wire_is_clean(""));
    }

    #[test]
    fn scan_ledger_finds_orphans_and_next_seq() {
        let text = concat!(
            r#"{"rec":"start","sid":"a","seq":1,"ts":1}"#,
            "\n",
            r#"{"rec":"end","sid":"a","ts":2,"reason":"clean"}"#,
            "\n",
            r#"{"rec":"start","sid":"b","seq":2,"ts":3}"#,
            "\n",
            r#"{"rec":"start","sid":"c","seq":3,"ts":4}"#,
            "\n",
            r#"{"rec":"end","sid":"c","ts":5,"reason":"panic"}"#,
            "\n",
            "not json at all\n",
        );
        let (orphans, next) = scan_ledger_text(text);
        assert_eq!(orphans, vec!["b".to_string()], "only b started without ending");
        assert_eq!(next, 4, "sequence continues past the highest seen");
    }

    #[test]
    fn scan_ledger_of_nothing_starts_at_one() {
        let (orphans, next) = scan_ledger_text("");
        assert!(orphans.is_empty());
        assert_eq!(next, 1);
    }

    #[test]
    fn surface_kind_is_a_closed_set() {
        let k = surface_kind();
        assert!(
            matches!(k, "editor" | "headless" | "client" | "server"),
            "surface tag {k} escaped the closed set"
        );
        assert!(!k.contains('/') && !k.contains('\\'), "no path may reach the ledger");
    }

    #[test]
    fn today_stamp_is_yyyymmdd() {
        let s = today_stamp();
        assert_eq!(s.len(), 8, "stamp {s} should be YYYYMMDD");
        assert!(s.chars().all(|c| c.is_ascii_digit()));
        assert!(s.starts_with("20"), "stamp {s} should be this century");
    }
}
