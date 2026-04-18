//! Claude call audit log — constitutional requirement (KL-10 + Section 7.3).
//!
//! Every Claude API call is written as a discrete `.log.toml` file inside
//! `{SpaceRoot}/SoulService/Logs/`. The folder auto-appears in the Explorer
//! under SoulService with a log-stack icon, so users can inspect the full
//! chain of AI decisions made on behalf of their Space without leaving the
//! editor. No cap, no pruning — the trail is the point.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The currently-loaded Space's root path, updated by a Bevy system whenever
/// [`crate::space::SpaceRoot`] changes. The audit writer reads this so it
/// doesn't need to be threaded through every Claude call site.
static CURRENT_SPACE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Called from a Bevy sync system whenever SpaceRoot changes.
pub fn set_current_space(path: Option<PathBuf>) {
    if let Ok(mut g) = CURRENT_SPACE.lock() {
        *g = path;
    }
}

/// Convenience wrapper — log a call against the currently-active Space.
/// No-op when no Space is loaded (e.g. pre-login screens).
pub fn log_claude_call(record: &ClaudeCallRecord) {
    let space = match CURRENT_SPACE.lock() {
        Ok(g) => g.clone(),
        Err(_) => return,
    };
    if let Some(space) = space {
        write_call_record(&space, record);
    }
}

/// One Claude call, serialised to TOML.
#[derive(Debug, Clone)]
pub struct ClaudeCallRecord {
    /// RFC 3339 timestamp when the call was issued.
    pub timestamp: String,
    /// Model id (e.g. "claude-sonnet-4-6-20250514").
    pub model: String,
    /// Which subsystem made the call — "soul-build", "workshop",
    /// "summarize", "vision-gen", etc. Free-form but should stay stable.
    pub caller: String,
    /// Full prompt text (no truncation — audit chain is load-bearing).
    pub prompt: String,
    /// Full response text.
    pub response: String,
    /// Input tokens from the API response's `usage` block.
    pub tokens_input: u32,
    /// Output tokens from the API response's `usage` block.
    pub tokens_output: u32,
    /// End-to-end duration of the API call.
    pub duration_ms: u64,
}

/// Resolve the Logs folder path for the currently-active Space, creating
/// the `_instance.toml` scaffold on first use so it appears in the Explorer.
pub fn logs_dir_for_space(space_root: &Path) -> PathBuf {
    let soul_svc = space_root.join("SoulService");
    let logs_dir = soul_svc.join("Logs");
    let _ = std::fs::create_dir_all(&logs_dir);

    // Ensure SoulService has a `_service.toml` so the service itself renders.
    // The per-service loader already handles missing files gracefully, so we
    // only write if absent — never clobber user edits.
    let svc_toml = soul_svc.join("_service.toml");
    if !svc_toml.exists() {
        let _ = std::fs::write(&svc_toml, SOUL_SERVICE_TOML);
    }

    // Ensure Logs/_instance.toml exists with a `log` icon hint so the
    // Explorer renders a log-stack glyph instead of a generic folder.
    let logs_toml = logs_dir.join("_instance.toml");
    if !logs_toml.exists() {
        let _ = std::fs::write(&logs_toml, LOGS_FOLDER_TOML);
    }

    logs_dir
}

/// Append one call record as a fresh `.log.toml` file. Filename format:
/// `YYYY-MM-DDThh-mm-ss_<caller>_<short-hash>.log.toml` — sortable,
/// unambiguous, one-file-per-call so each entry shows up individually in
/// the Explorer tree.
pub fn write_call_record(space_root: &Path, record: &ClaudeCallRecord) {
    let dir = logs_dir_for_space(space_root);

    // Short hash from prompt+response so repeated identical calls get
    // distinct filenames via timestamp but similar calls visually cluster.
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        record.prompt.hash(&mut h);
        record.response.hash(&mut h);
        format!("{:08x}", h.finish() as u32)
    };

    // Convert RFC 3339 timestamp → filesystem-safe chunk.
    let ts_safe: String = record.timestamp.chars()
        .map(|c| if c == ':' { '-' } else { c })
        .collect();

    let caller_safe: String = record.caller.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();

    let filename = format!("{}_{}_{}.log.toml", ts_safe, caller_safe, hash);
    let path = dir.join(filename);

    let body = format!(
        "[metadata]\n\
         class_name = \"ClaudeCallLog\"\n\
         archivable = true\n\
         created = \"{ts}\"\n\
         last_modified = \"{ts}\"\n\
         \n\
         [call]\n\
         timestamp = \"{ts}\"\n\
         model = \"{model}\"\n\
         caller = \"{caller}\"\n\
         tokens_input = {ti}\n\
         tokens_output = {to}\n\
         duration_ms = {d}\n\
         \n\
         [prompt]\n\
         text = '''\n{prompt}\n'''\n\
         \n\
         [response]\n\
         text = '''\n{response}\n'''\n",
        ts = record.timestamp,
        model = record.model,
        caller = record.caller,
        ti = record.tokens_input,
        to = record.tokens_output,
        d = record.duration_ms,
        prompt = escape_triple_quote(&record.prompt),
        response = escape_triple_quote(&record.response),
    );

    if let Err(e) = std::fs::write(&path, body) {
        bevy::log::warn!("Failed to write Claude audit log {:?}: {}", path, e);
    }
}

/// TOML multi-line literal strings can't contain triple single-quotes. This
/// is vanishingly rare in Claude output but defensively handled.
fn escape_triple_quote(s: &str) -> String {
    s.replace("'''", "''\\''")
}

/// Default SoulService scaffold — class_name + icon so the service renders
/// in the Explorer even when the user hasn't customised it.
const SOUL_SERVICE_TOML: &str = r#"[service]
class_name = "SoulService"
icon = "soulservice"
description = "AI-assisted script authoring and audit log."
can_have_children = true

[metadata]
id = "soulservice-service"
"#;

/// Logs folder scaffold — sets the log-stack icon and marks the folder so
/// the file loader treats it as a leaf container (no recursion into each
/// .log.toml as though they were instances).
const LOGS_FOLDER_TOML: &str = r#"[metadata]
class_name = "Folder"
archivable = true
name = "Logs"

[folder]
icon = "log"
description = "Claude API call audit trail — constitutional requirement (KL-10)."
"#;
