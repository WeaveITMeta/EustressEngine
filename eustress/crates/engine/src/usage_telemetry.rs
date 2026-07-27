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
        "Eustress counts which tools you click (never your content) to decide what to build next. \
         Turn it off any time in Settings ▸ Notifications ▸ Privacy.",
    ));
}

pub struct UsageTelemetryPlugin;

impl Plugin for UsageTelemetryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UsageTelemetry>()
            .add_systems(Update, (flush_usage_telemetry, flush_on_exit, sync_and_notice, startup_drain));
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
    fn today_stamp_is_yyyymmdd() {
        let s = today_stamp();
        assert_eq!(s.len(), 8, "stamp {s} should be YYYYMMDD");
        assert!(s.chars().all(|c| c.is_ascii_digit()));
        assert!(s.starts_with("20"), "stamp {s} should be this century");
    }
}
