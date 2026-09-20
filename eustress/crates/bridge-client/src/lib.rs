//! Synchronous client for the engine's TCP JSON-RPC "Engine Bridge".
//!
//! The engine (both the windowed `eustress-engine` and the headless
//! `eustress-headless`) binds a localhost listener (`127.0.0.1:<port>`)
//! and writes the chosen port to `<universe>/.eustress/engine.port` — the
//! same sentinel convention IDEs use to find the LSP (`.eustress/lsp.port`).
//! Sibling processes (the MCP server, the `eustress` CLI, future plugins)
//! discover the bridge by reading that file.
//!
//! Wire format (mirrors `eustress_engine::engine_bridge::protocol`): one
//! newline-terminated JSON line per direction.
//!
//! Request frame:
//! ```json
//! {"jsonrpc":"2.0","id":1,"method":"ecs.inspect","params":{"limit":10}}
//! ```
//! Response frame:
//! ```json
//! {"jsonrpc":"2.0","id":1,"result":{...}}        // success
//! {"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"..."}} // error
//! ```
//!
//! Kept deliberately synchronous + std-only (`std::net::TcpStream`): every
//! consumer's call site is a single short request/response round-trip, so
//! pulling in an async runtime here would buy nothing and would make this
//! crate a heavier add for the CLI. Every failure path returns a friendly
//! `Err(String)` — this function never panics, so a caller can surface
//! "engine isn't running" cleanly (to an AI, to a terminal, wherever).

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

/// How long to wait on connect before giving up. Connecting to a live
/// localhost listener is immediate; this only bounds the wait on a stale
/// port file.
const TIMEOUT: Duration = Duration::from_secs(2);

/// Default reply deadline. Most bridge methods are answered on the engine's
/// next frame, so 2 s is generous headroom.
///
/// It is NOT enough for every method: `sim.step` runs up to 10 000 `FixedMain`
/// schedules synchronously on the Bevy main thread before replying, which
/// takes far longer than a frame. Callers issuing work like that must pass a
/// deadline that matches — see [`call_engine_with_timeout`].
pub const DEFAULT_REPLY_TIMEOUT: Duration = TIMEOUT;

/// Friendly, human/AI-readable message for the "no live engine" case.
/// Returned for every discovery/connection failure so the caller is told
/// to start the engine (or `eustress-headless`) rather than seeing a raw
/// OS error.
fn not_running(detail: &str) -> String {
    format!(
        "Eustress engine is not running (no live bridge at .eustress/engine.port). \
         Open the engine or run eustress-headless and retry. [{detail}]"
    )
}

/// Read a port from `port_path`, parse it, and open a TCP connection to the
/// bridge (short timeout so a dead engine fails fast). Used for both the
/// per-universe port file and the global workspace-root fallback.
fn connect_via_port_file(port_path: &Path) -> Result<TcpStream, String> {
    let raw = std::fs::read_to_string(port_path)
        .map_err(|_| not_running(&format!("no port file at {}", port_path.display())))?;
    let port: u16 = raw
        .trim()
        .parse()
        .map_err(|_| not_running(&format!("invalid port file contents: {:?}", raw.trim())))?;
    let addr = format!("127.0.0.1:{port}");
    let sock_addr: SocketAddr = addr
        .parse()
        .map_err(|e| format!("internal: bad bridge address {addr}: {e}"))?;
    TcpStream::connect_timeout(&sock_addr, TIMEOUT)
        .map_err(|e| not_running(&format!("connect {addr} failed: {e}")))
}

/// Call one bridge method and return its `result` value.
///
/// `universe_dir` is the Universe root; the port file lives at
/// `<universe_dir>/.eustress/engine.port`.
///
/// On success returns the JSON-RPC `result` object. On any failure —
/// missing port file, unparseable port, connection refused, timeout,
/// malformed response, or a JSON-RPC `error` from the engine — returns a
/// clear `Err(String)`.
pub fn call_engine(universe_dir: &Path, method: &str, params: Value) -> Result<Value, String> {
    call_engine_with_timeout(universe_dir, method, params, DEFAULT_REPLY_TIMEOUT)
}

/// [`call_engine`] with an explicit deadline for the engine's reply.
///
/// Use this for methods that do real work before answering (`sim.step`,
/// long captures). Under the default deadline those calls report
/// "engine is not running" — a read timeout on an established connection is
/// indistinguishable from a dead socket unless you separate the two, which is
/// exactly backwards: the engine is not down, it is busy doing what you asked.
/// The connect deadline stays short regardless, so a genuinely absent engine
/// still fails fast.
pub fn call_engine_with_timeout(
    universe_dir: &Path,
    method: &str,
    params: Value,
    reply_timeout: Duration,
) -> Result<Value, String> {
    // ── Discover the port + connect ──────────────────────────────────
    // Try the configured universe's port file first; if it is missing or its
    // engine isn't answering, fall back to the GLOBAL port file at the shared
    // Eustress workspace root (the engine writes both). This lets a caller
    // find the live engine even when it launched into a DIFFERENT universe
    // than the one it's configured for.
    //
    // NOTE: this discovery is per-Universe and therefore single-instance —
    // two engines open on two Spaces of the SAME Universe overwrite each
    // other's port file. Anything driving several instances at once must
    // address them by port instead: see [`call_port`] and [`list_instances`].
    let universe_port = universe_dir.join(".eustress").join("engine.port");
    let global_port = universe_dir
        .parent()
        .map(|ws| ws.join(".eustress").join("engine.port"));

    let stream = match connect_via_port_file(&universe_port) {
        Ok(s) => s,
        Err(primary_err) => match global_port {
            // On global failure, surface the PRIMARY (per-universe) error — it's
            // the more relevant "your configured universe has no live engine".
            Some(gp) => connect_via_port_file(&gp).map_err(|_| primary_err)?,
            None => return Err(primary_err),
        },
    };
    round_trip(stream, method, params, reply_timeout)
}

/// Call one bridge method on an engine instance whose port is already known
/// — the multi-instance path. Bypasses port-file discovery entirely, so it
/// is the only unambiguous way to reach ONE specific engine when several
/// are running (get ports from [`list_instances`]).
pub fn call_port(port: u16, method: &str, params: Value) -> Result<Value, String> {
    call_port_with_timeout(port, method, params, DEFAULT_REPLY_TIMEOUT)
}

/// [`call_port`] with an explicit reply deadline (see
/// [`call_engine_with_timeout`] for when that matters).
pub fn call_port_with_timeout(
    port: u16,
    method: &str,
    params: Value,
    reply_timeout: Duration,
) -> Result<Value, String> {
    let addr = format!("127.0.0.1:{port}");
    let sock_addr: SocketAddr = addr
        .parse()
        .map_err(|e| format!("internal: bad bridge address {addr}: {e}"))?;
    let stream = TcpStream::connect_timeout(&sock_addr, TIMEOUT)
        .map_err(|e| not_running(&format!("connect {addr} failed: {e}")))?;
    round_trip(stream, method, params, reply_timeout)
}

/// One JSON-RPC request/response exchange over an already-connected stream.
fn round_trip(
    stream: TcpStream,
    method: &str,
    params: Value,
    reply_timeout: Duration,
) -> Result<Value, String> {
    stream
        .set_read_timeout(Some(reply_timeout))
        .map_err(|e| format!("internal: set_read_timeout failed: {e}"))?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(|e| format!("internal: set_write_timeout failed: {e}"))?;

    // ── Send one newline-terminated request frame ────────────────────
    //
    // `id` is a constant 1: this is a single synchronous round-trip on a
    // fresh connection, so there are no concurrent requests to
    // disambiguate. The engine echoes it back; we don't bother checking.
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    });
    let mut line = serde_json::to_string(&request)
        .map_err(|e| format!("internal: failed to encode request: {e}"))?;
    line.push('\n');

    let mut writer = stream
        .try_clone()
        .map_err(|e| not_running(&format!("socket clone failed: {e}")))?;
    writer
        .write_all(line.as_bytes())
        .map_err(|e| not_running(&format!("write to bridge failed: {e}")))?;
    writer
        .flush()
        .map_err(|e| not_running(&format!("flush to bridge failed: {e}")))?;

    // ── Read exactly one newline-terminated response frame ───────────
    //
    // We are past `connect`, so the engine demonstrably exists. A failure
    // here is a busy or wedged engine, never a missing one — reporting it as
    // "not running" sends the caller off to start a process that is already
    // up. `sim.step` with a large tick count is the common case.
    let mut reader = BufReader::new(stream);
    let mut resp_line = String::new();
    let n = reader.read_line(&mut resp_line).map_err(|e| {
        if matches!(
            e.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ) {
            format!(
                "Eustress engine accepted the connection but did not answer '{method}' within \
                 {:.1}s. It is running and busy — long operations (e.g. sim.step with many \
                 ticks) block the engine's main thread. Retry with fewer ticks or a longer \
                 deadline.",
                reply_timeout.as_secs_f32()
            )
        } else {
            not_running(&format!("read from bridge failed: {e}"))
        }
    })?;
    if n == 0 {
        return Err(not_running("bridge closed the connection without responding"));
    }

    // ── Parse the BridgeResponse and unwrap result / error ───────────
    let resp: Value = serde_json::from_str(resp_line.trim())
        .map_err(|e| format!("bridge returned malformed JSON: {e} — raw: {}", resp_line.trim()))?;

    if let Some(err) = resp.get("error") {
        let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("(no message)");
        return Err(format!("engine bridge error {code}: {msg}"));
    }

    match resp.get("result") {
        Some(result) => Ok(result.clone()),
        None => Err(format!(
            "bridge response had neither result nor error: {}",
            resp_line.trim()
        )),
    }
}

/// Discover the bridge's `engine.port` for `universe_dir` and return the
/// path it read (or would read) — used by callers that want to report
/// "which port file" without making an RPC call (e.g. a `bridge status`
/// command).
pub fn port_file_path(universe_dir: &Path) -> std::path::PathBuf {
    universe_dir.join(".eustress").join("engine.port")
}

// ─────────────────────────────────────────────────────────────────────────────
// Instance registry — one record per running engine, keyed by PID
// ─────────────────────────────────────────────────────────────────────────────
//
// The per-Universe `engine.port` file is a single slot: the last engine to
// start owns it, and the first to exit deletes it. That is fine for the
// one-Studio-at-a-time case it was built for, and wrong for an agent that
// opens several Spaces of one Universe at once. So every engine ALSO writes
// `<workspace>/.eustress/instances/<pid>.json` — collision-free by
// construction (a PID is unique among live processes) and enumerable, so an
// orchestrator can list what is running and drive each one by its own port
// via [`call_port`]. The engine removes its file on clean exit; a crash
// leaves a stale one, which [`list_instances`] prunes by pinging.

/// Which shell an instance is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstanceKind {
    /// The windowed editor (`eustress-engine`).
    Editor,
    /// The windowless simulator (`eustress-headless`).
    Headless,
}

impl InstanceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            InstanceKind::Editor => "editor",
            InstanceKind::Headless => "headless",
        }
    }
}

/// A running engine instance, as written to
/// `<workspace>/.eustress/instances/<pid>.json`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InstanceRecord {
    /// OS process id — the file name and the identity.
    pub pid: u32,
    /// Bridge TCP port on 127.0.0.1.
    pub port: u16,
    pub kind: InstanceKind,
    /// The Space this instance has open, if a Space is loaded. Updated by
    /// the engine on every runtime Space switch.
    #[serde(default)]
    pub space: Option<std::path::PathBuf>,
    /// The Universe root of that Space.
    #[serde(default)]
    pub universe: Option<std::path::PathBuf>,
    /// RFC 3339 timestamp of when the bridge came up.
    pub started_at: String,
}

/// `<workspace>/.eustress/instances` — the registry directory.
pub fn instances_dir(workspace_root: &Path) -> std::path::PathBuf {
    workspace_root.join(".eustress").join("instances")
}

/// The registry file for one PID.
pub fn instance_file_path(workspace_root: &Path, pid: u32) -> std::path::PathBuf {
    instances_dir(workspace_root).join(format!("{pid}.json"))
}

/// Read one instance record. `Err` on a missing or malformed file.
pub fn read_instance(path: &Path) -> Result<InstanceRecord, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse {}: {e}", path.display()))
}

/// Every instance record in the registry, in PID order, WITHOUT checking
/// whether the processes are still alive. Malformed files are skipped.
pub fn list_instances_unchecked(workspace_root: &Path) -> Vec<InstanceRecord> {
    let dir = instances_dir(workspace_root);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<InstanceRecord> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .filter_map(|p| read_instance(&p).ok())
        .collect();
    out.sort_by_key(|r| r.pid);
    out
}

/// Every LIVE instance: reads the registry and pings each recorded port,
/// deleting the record of any instance that no longer answers (a crashed
/// engine never gets to remove its own file). This is the call an
/// orchestrator should make before deciding what to drive.
pub fn list_instances(workspace_root: &Path) -> Vec<InstanceRecord> {
    list_instances_unchecked(workspace_root)
        .into_iter()
        .filter(|rec| {
            let alive = call_port(rec.port, "ping", serde_json::json!({})).is_ok();
            if !alive {
                let _ = std::fs::remove_file(instance_file_path(workspace_root, rec.pid));
            }
            alive
        })
        .collect()
}

/// The default Eustress workspace root (the parent of all Universes; where
/// the global `engine.port` and the instance registry live).
///
/// Resolution: `EUSTRESS_WORKSPACE` env var, else `<Documents>/Eustress`.
/// On Windows the LOCAL `%USERPROFILE%\Documents` is used in preference to
/// `dirs::document_dir()`, which on a OneDrive "Known Folder Move" install
/// resolves to the redirected `OneDrive\Documents` — a folder the engine's
/// own `space::default_documents_root` deliberately avoids, so this keeps
/// the CLI and the engine looking in the same place.
pub fn default_workspace_root() -> std::path::PathBuf {
    if let Ok(env_path) = std::env::var("EUSTRESS_WORKSPACE") {
        return std::path::PathBuf::from(env_path);
    }
    let documents = {
        #[cfg(target_os = "windows")]
        {
            dirs::home_dir()
                .map(|h| h.join("Documents"))
                .filter(|p| p.is_dir())
                .or_else(dirs::document_dir)
        }
        #[cfg(not(target_os = "windows"))]
        {
            dirs::document_dir()
        }
    };
    documents
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Eustress")
}
