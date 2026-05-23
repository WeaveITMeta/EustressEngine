//! Synchronous client for the engine's TCP JSON-RPC "engine bridge".
//!
//! The engine binds a localhost listener (`127.0.0.1:<port>`) and writes
//! the chosen port to `<universe>/.eustress/engine.port` — the same
//! sentinel convention IDEs use to find the LSP (`.eustress/lsp.port`).
//! Sibling processes (this MCP server, future plugins) discover the
//! bridge by reading that file.
//!
//! Wire format (mirrors `engine_bridge::protocol`): one newline-
//! terminated JSON line per direction.
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
//! Kept deliberately synchronous + std-only (`std::net::TcpStream`): the
//! MCP server's tool-dispatch path is synchronous, and pulling tokio into
//! a single short request/response round-trip would buy nothing. Every
//! failure path returns a friendly `Err(String)` — this function never
//! panics, so a tool can surface "engine isn't running" to the AI.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

/// How long to wait on connect / read before giving up. The engine
/// services bridge requests on its main frame, so a live engine answers
/// in well under a frame; 2 s is generous headroom that still fails fast
/// when nothing is listening.
const TIMEOUT: Duration = Duration::from_secs(2);

/// Friendly, AI-readable message for the "no live engine" case. Returned
/// for every discovery/connection failure so the model is told to open
/// the engine rather than seeing a raw OS error.
fn not_running(detail: &str) -> String {
    format!(
        "Eustress engine is not running (no live bridge at .eustress/engine.port). \
         Open the engine and retry. [{detail}]"
    )
}

/// Call one bridge method and return its `result` value.
///
/// `universe_dir` is the Universe root (`ToolContext::universe_root`);
/// the port file lives at `<universe_dir>/.eustress/engine.port`.
///
/// On success returns the JSON-RPC `result` object. On any failure —
/// missing port file, unparseable port, connection refused, timeout,
/// malformed response, or a JSON-RPC `error` from the engine — returns a
/// clear `Err(String)`.
pub fn call_engine(
    universe_dir: &Path,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    // ── Discover the port ────────────────────────────────────────────
    let port_path = universe_dir.join(".eustress").join("engine.port");
    let raw = std::fs::read_to_string(&port_path)
        .map_err(|_| not_running(&format!("no port file at {}", port_path.display())))?;
    let port: u16 = raw
        .trim()
        .parse()
        .map_err(|_| not_running(&format!("invalid port file contents: {:?}", raw.trim())))?;

    // ── Connect (short timeout so a dead engine fails fast) ──────────
    let addr = format!("127.0.0.1:{port}");
    let sock_addr: SocketAddr = addr
        .parse()
        .map_err(|e| format!("internal: bad bridge address {addr}: {e}"))?;
    let stream = TcpStream::connect_timeout(&sock_addr, TIMEOUT)
        .map_err(|e| not_running(&format!("connect {addr} failed: {e}")))?;
    stream
        .set_read_timeout(Some(TIMEOUT))
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
    let mut reader = BufReader::new(stream);
    let mut resp_line = String::new();
    let n = reader
        .read_line(&mut resp_line)
        .map_err(|e| not_running(&format!("read from bridge failed: {e}")))?;
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
